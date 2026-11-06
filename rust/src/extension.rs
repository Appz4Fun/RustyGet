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
    build_cipher_baseline(&names, &values)
}

fn script_config_cipher_baseline_cstrings(entries: &[(CString, CString)]) -> Vec<u8> {
    let mut names = Vec::with_capacity(entries.len());
    let mut values = Vec::with_capacity(entries.len());
    for (k, v) in entries {
        names.push(k.to_bytes());
        values.push(v.to_bytes());
    }
    build_cipher_baseline(&names, &values)
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
            crate::options::options_split_option_string(line)
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
                crate::options::options_split_option_string(trimmed)
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

    for raw_tok in extensions_list.split(|&b| b == b',' || b == b';') {
        let tok = trim_tokenizer_ws(raw_tok);
        if !tok.is_empty() && tok == target_name {
            return true;
        }
    }
    false
}

/// Computes the sorted index permutation for `ExtensionManager::Manager::Sort`.
pub fn ext_sort_indices(names: &[&[u8]], order_str: &[u8]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..names.len()).collect();
    fn trim_tok(mut s: &[u8]) -> &[u8] {
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

    let mut order: Vec<&[u8]> = Vec::new();
    for raw in order_str.split(|&b| b == b',' || b == b';') {
        let tok = trim_tok(raw);
        if !tok.is_empty() && !order.contains(&tok) {
            order.push(tok);
        }
    }

    if order.is_empty() {
        indices.sort_by(|&a, &b| names[a].cmp(names[b]));
        return indices;
    }

    let mut count = 0usize;
    for target in &order {
        if let Some(pos) = indices.iter().position(|&idx| names[idx] == *target) {
            indices.swap(count, pos);
            count += 1;
        }
    }

    indices[count..].sort_by(|&a, &b| names[a].cmp(names[b]));
    indices
}

/// Extracts the string value for `"<key>"` from `package-info.json` in `Maintenance::ReadPackageInfoStr`.
/// Returns `Some((value_start, value_len))` if found and `value_len < max_len`.
pub fn maintenance_extract_package_info_str(
    json: &[u8],
    key: &[u8],
    max_len: usize,
) -> Option<(usize, usize)> {
    let mut quoted_key = Vec::with_capacity(key.len() + 2);
    quoted_key.push(b'"');
    let key_trunc = if key.len() > 97 { &key[..97] } else { key };
    quoted_key.extend_from_slice(key_trunc);
    if quoted_key.len() < 99 {
        quoted_key.push(b'"');
    }

    let key_pos = json
        .windows(quoted_key.len())
        .position(|w| w == quoted_key.as_slice())?;
    let after_key = key_pos + quoted_key.len();
    let quote_open_rel = json[after_key..].iter().position(|&b| b == b'"')?;
    let val_start = after_key + quote_open_rel + 1;
    let quote_close_rel = json[val_start..].iter().position(|&b| b == b'"')?;
    if quote_close_rel >= max_len {
        return None;
    }
    Some((val_start, quote_close_rel))
}

/// Classifies a script output line (after stripping `m_prefixLen`) in `UpdateScriptController::AddMessage`:
/// - `0`: regular log line (does not start with `"[NZB] "`)
/// - `1`: `"[NZB] QUIT"` command
/// - `-1`: invalid `"[NZB] ..."` command
pub fn maintenance_classify_update_message(text: &[u8]) -> i32 {
    if !text.starts_with(b"[NZB] ") {
        0
    } else if &text[6..] == b"QUIT" {
        1
    } else {
        -1
    }
}

/// Classifies a script output line (after stripping `m_prefixLen`) in `UpdateInfoScriptController::AddMessage`:
/// - `0`: regular log line (does not start with `"[NZB] "`)
/// - `1`: `"[NZB] [UPDATEINFO]..."` command (payload starts at `text + 18`)
/// - `-1`: invalid `"[NZB] ..."` command
pub fn maintenance_classify_update_info_message(text: &[u8]) -> i32 {
    if !text.starts_with(b"[NZB] ") {
        0
    } else if text[6..].starts_with(b"[UPDATEINFO]") {
        1
    } else {
        -1
    }
}

/// Decodes a hex-encoded RSA-SHA256 signature line tail in `Signature::ReadSignature`
/// (starting after `sigTitle`, trimming 2 trailing bytes when `sig_len > 2`, and decoding
/// up to 256 bytes into `out_sig`). Returns `true` iff exactly 256 bytes were decoded.
pub fn maintenance_decode_hex_signature(hex_sig: &[u8], out_sig: &mut [u8; 256]) -> bool {
    let eff_len = if hex_sig.len() > 2 {
        hex_sig.len() - 2
    } else {
        hex_sig.len()
    };
    let slice = &hex_sig[..eff_len];
    let mut in_idx = 0;
    let mut out_idx = 0;
    while in_idx + 1 < slice.len() && out_idx < 256 {
        let c1 = slice[in_idx];
        let c2 = slice[in_idx + 1];
        in_idx += 2;
        let n1 = match c1 {
            b'0'..=b'9' => c1 - b'0',
            b'A'..=b'F' => c1 - b'A' + 10,
            b'a'..=b'f' => c1 - b'a' + 10,
            _ => 0,
        };
        let n2 = match c2 {
            b'0'..=b'9' => c2 - b'0',
            b'A'..=b'F' => c2 - b'A' + 10,
            b'a'..=b'f' => c2 - b'a' + 10,
            _ => 0,
        };
        out_sig[out_idx] = (n1 << 4).wrapping_add(n2);
        out_idx += 1;
    }
    out_idx == 256
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

use std::collections::BTreeMap;

pub const QUEUE_EVENT_NAMES: [&[u8]; 7] = [
    b"FILE_DOWNLOADED",
    b"URL_COMPLETED",
    b"NZB_MARKED",
    b"NZB_ADDED",
    b"NZB_NAMED",
    b"NZB_DOWNLOADED",
    b"NZB_DELETED",
];

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsQueueScriptCmd {
    /// - `0`: not an `[NZB] ` command (log normally)
    /// - `1`: `[NZB] NZBPR_<key>=<val>`
    /// - `2`: `[NZB] DIRECTORY=<dir>` (only when `event == qe_nzb_downloaded`)
    /// - `3`: `[NZB] MARK=BAD`
    /// - `-1`: invalid `[NZB] ` command
    pub kind: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub val_offset: usize,
    pub val_len: usize,
}

/// Parses a `[NZB] ...` line in `QueueScriptController::AddMessage`.
pub fn parse_queue_script_cmd(
    msg_text: &[u8],
    event: i32,
    qe_nzb_downloaded: i32,
) -> RsQueueScriptCmd {
    if !msg_text.starts_with(b"[NZB] ") {
        return RsQueueScriptCmd::default();
    }
    let body = &msg_text[6..];
    if body.starts_with(b"NZBPR_") {
        let kv = &body[6..];
        if let Some(eq) = kv.iter().position(|&b| b == b'=') {
            RsQueueScriptCmd {
                kind: 1,
                key_offset: 12,
                key_len: eq,
                val_offset: 12 + eq + 1,
                val_len: kv.len() - eq - 1,
            }
        } else {
            RsQueueScriptCmd {
                kind: -1,
                ..Default::default()
            }
        }
    } else if body.starts_with(b"DIRECTORY=") && event == qe_nzb_downloaded {
        RsQueueScriptCmd {
            kind: 2,
            key_offset: 6,
            key_len: 9,
            val_offset: 16,
            val_len: body.len() - 10,
        }
    } else if body.starts_with(b"MARK=BAD") {
        RsQueueScriptCmd {
            kind: 3,
            ..Default::default()
        }
    } else {
        RsQueueScriptCmd {
            kind: -1,
            ..Default::default()
        }
    }
}

/// Evaluates the `EventInterval` suppression check in `QueueScriptCoordinator::EnqueueScript`.
pub fn should_skip_queue_event_interval(
    event: i32,
    qe_file_downloaded: i32,
    event_interval: i32,
    cur_time: i64,
    queue_script_time: i64,
) -> bool {
    event == qe_file_downloaded
        && (event_interval == -1
            || (event_interval > 0
                && cur_time - queue_script_time > 0
                && ((cur_time - queue_script_time) as i32) < event_interval))
}

/// Checks if a parameter value is truthy (`"yes"`, `"on"`, `"1"`, case-insensitive).
fn is_param_enabled_value(val: &[u8]) -> bool {
    val.eq_ignore_ascii_case(b"yes") || val.eq_ignore_ascii_case(b"on") || val == b"1"
}

/// Evaluates `QueueScriptCoordinator::UsableScript`.
pub fn is_usable_queue_script(
    is_queue_script: bool,
    queue_events: &[u8],
    script_name: &[u8],
    event: i32,
    qe_url_completed: i32,
    param_names: &[&[u8]],
    param_values: &[&[u8]],
    url_extensions: &[u8],
) -> bool {
    if !is_queue_script {
        return false;
    }
    let event_name = if event >= 0 && (event as usize) < QUEUE_EVENT_NAMES.len() {
        QUEUE_EVENT_NAMES[event as usize]
    } else {
        b""
    };
    if !queue_events.is_empty()
        && !event_name.is_empty()
        && !queue_events
            .windows(event_name.len())
            .any(|w| w == event_name)
    {
        return false;
    }

    let param_count = param_names.len().min(param_values.len());
    for i in 0..param_count {
        let varname = param_names[i];
        if !varname.is_empty()
            && varname[0] != b'*'
            && varname[varname.len() - 1] == b':'
            && is_param_enabled_value(param_values[i])
        {
            let stripped = &varname[..varname.len() - 1];
            let truncated = if stripped.len() > 1023 {
                &stripped[..1023]
            } else {
                stripped
            };
            if truncated == script_name {
                return true;
            }
        }
    }

    if event == qe_url_completed && !url_extensions.is_empty() {
        for tok in url_extensions.split(|&b| b == b',' || b == b';') {
            let trimmed = trim_ascii_spaces(tok);
            if !trimmed.is_empty() && trimmed == script_name {
                return true;
            }
        }
    }

    false
}

fn trim_ascii_spaces(mut s: &[u8]) -> &[u8] {
    while let Some((&last, rest)) = s.split_last() {
        if last == b'\n' || last == b'\r' || last == b' ' || last == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    while let Some((&first, rest)) = s.split_first() {
        if first == b'\n' || first == b'\r' || first == b' ' || first == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

/// Evaluates whether a queued script item should be ignored/erased in `QueueScriptCoordinator::CheckQueue`.
pub fn should_ignore_queued_script_event(
    has_nzb_info: bool,
    delete_status: i32,
    ds_none: i32,
    mark_status: i32,
    ks_bad: i32,
    event: i32,
    qe_nzb_deleted: i32,
    qe_nzb_marked: i32,
) -> bool {
    !has_nzb_info
        || (delete_status != ds_none && event != qe_nzb_deleted && event != qe_nzb_marked)
        || (mark_status == ks_bad && event != qe_nzb_marked)
}

// ============================================================================
// 7. PostScript (`daemon/extension/PostScript.cpp`)
// ============================================================================

/// Builds the comma-separated list of enabled post-script names in `PostScriptController::Run`.
pub fn build_enabled_script_comma_list(
    param_names: &[&[u8]],
    param_values: &[&[u8]],
) -> Vec<u8> {
    let count = param_names.len().min(param_values.len());
    let mut out = Vec::new();
    for i in 0..count {
        let varname = param_names[i];
        if !varname.is_empty()
            && varname[0] != b'*'
            && varname[varname.len() - 1] == b':'
            && is_param_enabled_value(param_values[i])
        {
            out.extend_from_slice(&varname[..varname.len() - 1]);
            out.push(b',');
        }
    }
    out
}

/// Evaluates `PostScriptController::AnalyseExitCode`.
/// Returns `(script_status, message_action, request_par_check)` where `message_action`:
/// - `1`: info `"%s successful"`
/// - `2`: error `"%s failed"`
/// - `3`: info `"%s skipped"`
/// - `4`: error `"%s requested par-check/repair, but the collection was already checked"`
/// - `5`: info `"%s requested par-check/repair"`
/// - `6`: error `"%s failed (terminated with unknown status)"`
pub fn analyse_post_script_exit_code(
    exit_code: i32,
    par_status: i32,
    ps_skipped: i32,
    parcheck_enabled: bool,
    sr_none: i32,
    sr_failure: i32,
    sr_success: i32,
) -> (i32, i32, bool) {
    match exit_code {
        93 => (sr_success, 1, false),
        94 | -1 => (sr_failure, 2, false),
        95 => (sr_none, 3, false),
        92 if parcheck_enabled => {
            if par_status > ps_skipped {
                (sr_failure, 4, false)
            } else {
                (sr_success, 5, true)
            }
        }
        _ => (sr_failure, 6, false),
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsPostScriptCmd {
    /// - `0`: not an `[NZB] ` command
    /// - `1`: `[NZB] FINALDIR=<dir>`
    /// - `2`: `[NZB] DIRECTORY=<dir>`
    /// - `3`: `[NZB] NZBPR_<key>=<val>`
    /// - `4`: `[NZB] MARK=BAD`
    /// - `-1`: invalid `[NZB] ` command
    pub kind: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub val_offset: usize,
    pub val_len: usize,
}

/// Parses a `[NZB] ...` line in `PostScriptController::AddMessage`.
pub fn parse_post_script_cmd(msg_text: &[u8]) -> RsPostScriptCmd {
    if !msg_text.starts_with(b"[NZB] ") {
        return RsPostScriptCmd::default();
    }
    let body = &msg_text[6..];
    if body.starts_with(b"FINALDIR=") {
        RsPostScriptCmd {
            kind: 1,
            key_offset: 6,
            key_len: 8,
            val_offset: 15,
            val_len: body.len() - 9,
        }
    } else if body.starts_with(b"DIRECTORY=") {
        RsPostScriptCmd {
            kind: 2,
            key_offset: 6,
            key_len: 9,
            val_offset: 16,
            val_len: body.len() - 10,
        }
    } else if body.starts_with(b"NZBPR_") {
        let kv = &body[6..];
        if let Some(eq) = kv.iter().position(|&b| b == b'=') {
            RsPostScriptCmd {
                kind: 3,
                key_offset: 12,
                key_len: eq,
                val_offset: 12 + eq + 1,
                val_len: kv.len() - eq - 1,
            }
        } else {
            RsPostScriptCmd {
                kind: -1,
                ..Default::default()
            }
        }
    } else if body.starts_with(b"MARK=BAD") {
        RsPostScriptCmd {
            kind: 4,
            ..Default::default()
        }
    } else {
        RsPostScriptCmd {
            kind: -1,
            ..Default::default()
        }
    }
}

// ============================================================================
// 8. ScanScript (`daemon/extension/ScanScript.cpp`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsScanScriptCmd {
    /// - `0`: not an `[NZB] ` command
    /// - `1`: `[NZB] NZBNAME=<val>`
    /// - `2`: `[NZB] CATEGORY=<val>`
    /// - `3`: `[NZB] NZBPR_<key>=<val>`
    /// - `4`: `[NZB] PRIORITY=<int>`
    /// - `5`: `[NZB] TOP=<bool>`
    /// - `6`: `[NZB] PAUSED=<bool>`
    /// - `7`: `[NZB] DUPEKEY=<val>`
    /// - `8`: `[NZB] DUPESCORE=<int>`
    /// - `9`: `[NZB] DUPEMODE=<mode>` (`int_val` = 0 score, 1 all, 2 force)
    /// - `-1`: invalid `[NZB] ` command
    /// - `-2`: invalid value for `DUPEMODE` (`val_offset`/`val_len` point to value)
    pub kind: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub val_offset: usize,
    pub val_len: usize,
    pub int_val: i32,
}

fn c_atoi(bytes: &[u8]) -> i32 {
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    let mut sign: i64 = 1;
    if idx < bytes.len() {
        if bytes[idx] == b'-' {
            sign = -1;
            idx += 1;
        } else if bytes[idx] == b'+' {
            idx += 1;
        }
    }
    let mut acc: i64 = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        acc = acc
            .wrapping_mul(10)
            .wrapping_add((bytes[idx] - b'0') as i64);
        idx += 1;
    }
    (acc * sign) as i32
}

/// Parses a `[NZB] ...` line in `ScanScriptController::AddMessage`.
pub fn parse_scan_script_cmd(msg_text: &[u8]) -> RsScanScriptCmd {
    if !msg_text.starts_with(b"[NZB] ") {
        return RsScanScriptCmd::default();
    }
    let body = &msg_text[6..];
    if body.starts_with(b"NZBNAME=") {
        RsScanScriptCmd {
            kind: 1,
            val_offset: 14,
            val_len: body.len() - 8,
            ..Default::default()
        }
    } else if body.starts_with(b"CATEGORY=") {
        RsScanScriptCmd {
            kind: 2,
            val_offset: 15,
            val_len: body.len() - 9,
            ..Default::default()
        }
    } else if body.starts_with(b"NZBPR_") {
        let kv = &body[6..];
        if let Some(eq) = kv.iter().position(|&b| b == b'=') {
            RsScanScriptCmd {
                kind: 3,
                key_offset: 12,
                key_len: eq,
                val_offset: 12 + eq + 1,
                val_len: kv.len() - eq - 1,
                int_val: 0,
            }
        } else {
            RsScanScriptCmd {
                kind: -1,
                ..Default::default()
            }
        }
    } else if body.starts_with(b"PRIORITY=") {
        RsScanScriptCmd {
            kind: 4,
            val_offset: 15,
            val_len: body.len() - 9,
            int_val: c_atoi(&body[9..]),
            ..Default::default()
        }
    } else if body.starts_with(b"TOP=") {
        RsScanScriptCmd {
            kind: 5,
            val_offset: 10,
            val_len: body.len() - 4,
            int_val: if c_atoi(&body[4..]) != 0 { 1 } else { 0 },
            ..Default::default()
        }
    } else if body.starts_with(b"PAUSED=") {
        RsScanScriptCmd {
            kind: 6,
            val_offset: 13,
            val_len: body.len() - 7,
            int_val: if c_atoi(&body[7..]) != 0 { 1 } else { 0 },
            ..Default::default()
        }
    } else if body.starts_with(b"DUPEKEY=") {
        RsScanScriptCmd {
            kind: 7,
            val_offset: 14,
            val_len: body.len() - 8,
            ..Default::default()
        }
    } else if body.starts_with(b"DUPESCORE=") {
        RsScanScriptCmd {
            kind: 8,
            val_offset: 16,
            val_len: body.len() - 10,
            int_val: c_atoi(&body[10..]),
            ..Default::default()
        }
    } else if body.starts_with(b"DUPEMODE=") {
        let val = &body[9..];
        let mode = if val.eq_ignore_ascii_case(b"score") {
            0
        } else if val.eq_ignore_ascii_case(b"all") {
            1
        } else if val.eq_ignore_ascii_case(b"force") {
            2
        } else {
            return RsScanScriptCmd {
                kind: -2,
                val_offset: 15,
                val_len: val.len(),
                ..Default::default()
            };
        };
        RsScanScriptCmd {
            kind: 9,
            val_offset: 15,
            val_len: val.len(),
            int_val: mode,
            ..Default::default()
        }
    } else {
        RsScanScriptCmd {
            kind: -1,
            ..Default::default()
        }
    }
}

// ============================================================================
// 9. ScriptConfig (`daemon/extension/ScriptConfig.cpp`)
// ============================================================================

/// Builds the canonical versioned `v1\n` cipher baseline token in `ScriptConfig::CipherBaseline`.
pub fn build_cipher_baseline(names: &[&[u8]], values: &[&[u8]]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let count = names.len().min(values.len());
    let mut fields: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

    for i in 0..count {
        let raw_name = names[i];
        let mut name: Vec<u8> = raw_name
            .iter()
            .map(|&b| {
                if b.is_ascii_uppercase() {
                    b + (b'a' - b'A')
                } else {
                    b
                }
            })
            .collect();
        if !name.starts_with(b"server") {
            continue;
        }
        let dot = match name[6..].iter().position(|&b| b == b'.') {
            Some(pos) => 6 + pos,
            None => continue,
        };
        if dot == 6
            || name[6] == b'0'
            || !name[6..dot].iter().all(|&c| c.is_ascii_digit())
        {
            continue;
        }
        let suffix = &name[dot + 1..];
        if suffix != b"host" && suffix != b"port" && suffix != b"cipher" {
            continue;
        }
        let raw_val = values[i];
        let mut hex_val = Vec::with_capacity(raw_val.len() * 2);
        for &c in raw_val {
            hex_val.push(HEX[(c >> 4) as usize]);
            hex_val.push(HEX[(c & 15) as usize]);
        }
        fields.insert(std::mem::take(&mut name), hex_val);
    }

    let mut token = Vec::from(&b"v1\n"[..]);
    for (name, val) in fields {
        token.extend_from_slice(&name);
        token.push(b'=');
        token.extend_from_slice(&val);
        token.push(b'\n');
    }
    token
}

// ============================================================================
// 10. ScriptController (`daemon/util/ScriptController.cpp`)
// ============================================================================

/// Checks whether an environment variable string `var` matches the `NZBXX_...` pattern
/// filtered out in `EnvironmentStrings::InitFromCurrentProcess`.
pub fn is_nzbget_internal_env_var(var: &[u8]) -> bool {
    var.len() > 5 && var.starts_with(b"NZB") && var[5] == b'_'
}

/// Normalizes an environment variable name in `ScriptController::SetEnvVarSpecial`:
/// replaces `.:*!"$%&/()=`+~#'{}[]@- ` with `_` and converts ASCII letters to uppercase.
pub fn normalize_special_env_var_name(varname: &[u8]) -> Vec<u8> {
    const SPECIAL: &[u8] = b".:*!\"$%&/()=`+~#'{}[]@- ";
    varname
        .iter()
        .map(|&b| {
            let replaced = if SPECIAL.contains(&b) { b'_' } else { b };
            replaced.to_ascii_uppercase()
        })
        .collect()
}

/// Strips trailing `\n`, `\r`, and `' '` in-place and classifies the log prefix
/// in `ScriptController::ProcessOutput`.
/// Returns `(is_empty, message_kind, text_offset)` where `message_kind`:
/// - `0`: `mkInfo`
/// - `1`: `mkWarning`
/// - `2`: `mkError`
/// - `3`: `mkDetail`
/// - `4`: `mkDebug`
pub fn classify_script_output_line(line: &[u8]) -> ( usize, i32, usize) {
    let mut trimmed_len = line.len();
    while trimmed_len > 0 {
        let last = line[trimmed_len - 1];
        if last == b'\n' || last == b'\r' || last == b' ' {
            trimmed_len -= 1;
        } else {
            break;
        }
    }
    if trimmed_len == 0 {
        return (0, 0, 0);
    }
    let trimmed = &line[..trimmed_len];
    if trimmed.starts_with(b"[INFO] ") {
        (trimmed_len, 0, 7)
    } else if trimmed.starts_with(b"[WARNING] ") {
        (trimmed_len, 1, 10)
    } else if trimmed.starts_with(b"[ERROR] ") {
        (trimmed_len, 2, 8)
    } else if trimmed.starts_with(b"[DETAIL] ") {
        (trimmed_len, 3, 9)
    } else if trimmed.starts_with(b"[DEBUG] ") {
        (trimmed_len, 4, 8)
    } else {
        (trimmed_len, 0, 0)
    }
}

/// Evaluates `ScriptController::CheckEnvSize`.
/// Returns `Some(exceeded_total)` if cumulative environment size exceeds `limit`, or `None`.
pub fn check_env_size_limit(env_lens: &[usize], ptr_size: usize, limit: usize) -> Option<usize> {
    if limit == 0 {
        return None;
    }
    let mut total = 0usize;
    for &len in env_lens {
        total = total.saturating_add(len).saturating_add(1).saturating_add(ptr_size);
        if total > limit {
            return Some(total);
        }
    }
    None
}

/// Resolves whether a single-argument script should use `found_executor` as `m_cmdLine`
/// and whether to prepend `found_executor` to `m_cmdArgs` (`ScriptController::PrepareArgs`).
pub fn script_resolve_single_arg_executor(
    has_found: bool,
    found_executor: &[u8],
    arg0: &[u8],
) -> (bool, bool) {
    (has_found, has_found && found_executor != arg0)
}

/// Formats `"<name>=<value>"` for `ScriptController::SetEnvVar`.
pub fn script_ctrl_format_env_assignment(name: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 1 + value.len());
    out.extend_from_slice(name);
    out.push(b'=');
    out.extend_from_slice(value);
    out
}

/// Formats `"<prefix>_<name>"` for `ScriptController::SetEnvVarSpecial`.
pub fn script_ctrl_format_prefixed_varname(prefix: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 1 + name.len());
    out.extend_from_slice(prefix);
    out.push(b'_');
    out.extend_from_slice(name);
    out
}

/// Formats `"<prefix>: <text>"` for `ScriptController::PrintMessage`.
pub fn script_format_prefixed_message(prefix: &[u8], text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 2 + text.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(b": ");
    out.extend_from_slice(text);
    out
}

/// Evaluates whether `ScriptController::Terminate` succeeded.
pub fn script_ctrl_is_terminate_ok(child_pid: i32, kill_res: i32, completed: bool) -> bool {
    child_pid <= 0 || kill_res == 0 || completed
}

/// Returns true when `ScriptController::Break` has an active uncompleted process to signal.
pub fn script_ctrl_can_send_break(pid: i32, completed: bool) -> bool {
    pid > 0 && !completed
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
