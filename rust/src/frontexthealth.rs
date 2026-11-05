// Copyright (C) 2026
// Hot-path Rust implementations for Frontend, NCursesFrontend, ColoredFrontend,
// LoggableFrontend, Maintenance, ExtensionManager, NntpServer, and SystemHealth validators.

/// Returns `true` when `control_ip` is `"0.0.0.0"`, which `Frontend` maps to `"127.0.0.1"`.
pub fn frontend_is_any_ip(control_ip: &[u8]) -> bool {
    control_ip == b"0.0.0.0"
}

/// Computes the starting message index and whether to call `PrintSkip()` in `LoggableFrontend::Update`.
/// Returns `(start_index, should_skip)`.
pub fn frontend_calc_log_start_index(needed_first_id: i32, first_msg_id: i32) -> (i32, bool) {
    let start = needed_first_id.wrapping_sub(first_msg_id).wrapping_add(1);
    if start < 0 {
        (0, true)
    } else {
        (start, false)
    }
}

/// Computes `(hours, minutes, seconds)` for the remaining download time in
/// `ColoredFrontend::PrintStatus` and `NCursesFrontend::PrintStatus`.
/// Returns `None` when `stand_by` is true, `current_speed <= 0`, or `pause_download` is true.
pub fn frontend_calc_remaining_hms(
    remaining_size: i64,
    current_speed: i64,
    stand_by: bool,
    pause_download: bool,
) -> Option<(i32, i32, i32)> {
    let speed = if stand_by { 0 } else { current_speed };
    if speed > 0 && !pause_download {
        let remain_sec = remaining_size / speed;
        let h = (remain_sec / 3600) as i32;
        let m = ((remain_sec % 3600) / 60) as i32;
        let s = (remain_sec % 60) as i32;
        Some((h, m, s))
    } else {
        None
    }
}

/// Calculates clamped column width and window heights in `NCursesFrontend::CalcWindowSizes`:
/// returns `(clamped_cols, queue_win_height, queue_win_client_height, messages_win_top, messages_win_height, messages_win_client_height)`.
pub fn ncurses_calc_window_sizes(
    nr_rows: i32,
    nr_columns: i32,
    max_screen_width: i32,
    queue_win_pct: i32,
    queue_size: i32,
) -> (i32, i32, i32, i32, i32, i32) {
    let clamped_cols = nr_columns.clamp(9, max_screen_width);
    let mut queue_win_height = (nr_rows - 2) * queue_win_pct / 100;
    if queue_win_height - 1 > queue_size {
        queue_win_height = if queue_size > 0 { queue_size + 1 } else { 2 };
    }
    let mut queue_win_client_height = queue_win_height - 1;
    if queue_win_client_height < 0 {
        queue_win_client_height = 0;
    }
    let messages_win_top = queue_win_height;
    let messages_win_height = nr_rows - queue_win_height - 2;
    let mut messages_win_client_height = messages_win_height - 1;
    if messages_win_client_height < 0 {
        messages_win_client_height = 0;
    }
    (
        clamped_cols,
        queue_win_height,
        queue_win_client_height,
        messages_win_top,
        messages_win_height,
        messages_win_client_height,
    )
}

/// Computes the updated `(selected_entry, scroll_offset)` in `NCursesFrontend::SetCurrentQueueEntry`.
pub fn ncurses_calc_scroll_and_selection(
    mut entry: i32,
    queue_size: i32,
    mut scroll_offset: i32,
    client_height: i32,
) -> (i32, i32) {
    if entry < 0 {
        entry = 0;
    } else if entry > queue_size - 1 {
        entry = queue_size - 1;
    }

    if entry > scroll_offset + client_height || entry < scroll_offset - client_height {
        scroll_offset = entry - client_height / 2;
    } else if entry < scroll_offset {
        scroll_offset -= client_height;
    } else if entry >= scroll_offset + client_height {
        scroll_offset += client_height;
    }

    if scroll_offset > queue_size - client_height {
        scroll_offset = queue_size - client_height;
    }
    if scroll_offset < 0 {
        scroll_offset = 0;
    }

    (entry, scroll_offset)
}

/// Decomposes `time_sec` into `(days, hours, minutes, seconds)` for `NCursesFrontend::PrintTopHeader`.
pub fn ncurses_calc_top_header_time(time_sec: i32) -> (i32, i32, i32, i32) {
    let d = time_sec / 3600 / 24;
    let h = (time_sec % (3600 * 24)) / 3600;
    let m = (time_sec % 3600) / 60;
    let s = time_sec % 60;
    (d, h, m, s)
}

/// Resolves the pause/resume/extra-par file edit action in `NCursesFrontend::EditQueue`
/// when `m_groupFiles` is enabled and `action == eaFilePause` (`4`).
/// Returns `(resolved_file_action, new_last_pause_pars)` where `resolved_file_action` is:
/// - `5` (`eaFileResume`) when `remaining_size == paused_size`
/// - `8` (`eaFilePauseExtraPars`) when `paused_size == 0 && remaining_par_count > 0 && !(last_pause_pars && last_edit_entry == selected_entry)`
/// - `4` (`eaFilePause`) otherwise
pub fn ncurses_resolve_group_pause_action(
    action: i32,
    remaining_size: i64,
    paused_size: i64,
    remaining_par_count: i32,
    last_pause_pars: bool,
    last_edit_entry: i32,
    selected_entry: i32,
) -> (i32, bool) {
    // DownloadQueue::eaFilePause == 4, eaFileResume == 5, eaFilePauseExtraPars == 8
    if action != 4 {
        return (action, last_pause_pars);
    }
    if remaining_size == paused_size {
        (5, last_pause_pars)
    } else if paused_size == 0
        && remaining_par_count > 0
        && !(last_pause_pars && last_edit_entry == selected_entry)
    {
        (8, true)
    } else {
        (4, false)
    }
}

/// Processes a single key in `NCursesFrontend::UpdateInput` when `m_inputMode == downloadRate`.
/// Returns `(step_action, new_index, new_value)`:
/// - `step_action == 0`: continue reading keys with updated `(new_index, new_value)`
/// - `step_action == 1`: Enter pressed (`10` or `13`) -> apply `input_value * 1024`, switch to `normal`, and return
/// - `step_action == 2`: Escape pressed (`27`) -> switch to `normal` and return
pub fn ncurses_step_rate_input(
    key: i32,
    key_backspace: i32,
    mut input_number_index: i32,
    mut input_value: i32,
) -> (i32, i32, i32) {
    if input_number_index < 5 && key >= (b'0' as i32) && key <= (b'9' as i32) {
        input_value = (input_value * 10) + (key - (b'0' as i32));
        input_number_index += 1;
        (0, input_number_index, input_value)
    } else if key == 10 || key == 13 {
        (1, input_number_index, input_value)
    } else if key == 27 {
        (2, input_number_index, input_value)
    } else if input_number_index > 0 && key == key_backspace {
        let remain = input_value % 10;
        input_value = (input_value - remain) / 10;
        input_number_index -= 1;
        (0, input_number_index, input_value)
    } else {
        (0, input_number_index, input_value)
    }
}

// ============================================================================
// Maintenance hot paths
// ============================================================================

/// Extracts the string value for `"<key>"` from `package-info.json` in `Maintenance::ReadPackageInfoStr`.
/// Returns `Some((value_start, value_len))` if found and `value_len < max_len`.
pub fn maintenance_extract_package_info_str(
    json: &[u8],
    key: &[u8],
    max_len: usize,
) -> Option<(usize, usize)> {
    // Build `"<key>"` matching BString<100> keyStr("\"%s\"", key)
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

// ============================================================================
// ExtensionManager hot paths
// ============================================================================

/// Returns `true` when `filename` starts with `'.'` or `'_'` (or is empty) in
/// `ExtensionManager::Manager::LoadExtensionDir`.
pub fn ext_should_skip_dir_entry(filename: &[u8]) -> bool {
    matches!(filename.first(), None | Some(b'.') | Some(b'_'))
}

/// Returns the stem length of `filename` up to the last `'.'` (or `filename.len()` if no `'.'`)
/// for `ExtensionManager::Manager::GetExtensionName`.
pub fn ext_get_stem_len(filename: &[u8]) -> usize {
    filename
        .iter()
        .rposition(|&b| b == b'.')
        .unwrap_or(filename.len())
}

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

/// Checks whether `target_name` appears in a `",;"`-separated extension list
/// (matching `Tokenizer(extensions, ",;")` in `ExtensionManager::Manager::CreateTasks`).
pub fn ext_is_in_list(extensions_list: &[u8], target_name: &[u8]) -> bool {
    for raw_tok in extensions_list.split(|&b| b == b',' || b == b';') {
        let tok = trim_tokenizer_ws(raw_tok);
        if !tok.is_empty() && tok == target_name {
            return true;
        }
    }
    false
}

// ============================================================================
// SystemHealth validator hot paths
// ============================================================================

/// Evaluates `SystemHealth::CheckPassword`:
/// - `1`: empty (`Warning`)
/// - `2`: length < 8 (`Info`)
/// - `0`: valid (`Ok`)
pub fn health_check_password(password: &[u8]) -> i32 {
    if password.is_empty() {
        1
    } else if password.len() < 8 {
        2
    } else {
        0
    }
}

/// Evaluates `SystemHealth::Network::ValidPort`: returns `true` when `1 <= port <= 65535`.
pub fn health_valid_port(port: i32) -> bool {
    (1..=65535).contains(&port)
}

/// Evaluates `SystemHealth::Security::ControlIpValidator::Validate`:
/// - `-1`: empty (`Error`)
/// - `1`: `"0.0.0.0"` (`Info`)
/// - `0`: `"127.0.0.1"` or standard IP (`Ok`)
/// - `-2`: Unix domain socket path on Windows (`Error`)
/// - `2`: Unix domain socket path on non-Windows (`Info`)
pub fn health_validate_control_ip(ip: &[u8], is_win32: bool) -> i32 {
    if ip.is_empty() {
        return -1;
    }
    if ip == b"0.0.0.0" {
        return 1;
    }
    if ip == b"127.0.0.1" {
        return 0;
    }
    if ip[0] == b'/' || ip[0] == b'.' {
        return if is_win32 { -2 } else { 2 };
    }
    0
}

/// Evaluates `AddPasswordValidator::Validate` and `RestrictedPasswordValidator::Validate`:
/// - `1`: user non-empty and password empty (`Warning`)
/// - `2`: user empty and password non-empty (`Info`)
/// - `0`: `Ok`
pub fn health_validate_user_pass_pair(user: &[u8], pass: &[u8]) -> i32 {
    if !user.is_empty() && pass.is_empty() {
        1
    } else if user.is_empty() && !pass.is_empty() {
        2
    } else {
        0
    }
}

/// Evaluates `FormAuthValidator::Validate`:
/// - `0`: `Ok`
/// - `1`: `FormAuth` enabled while `SecureControl` is disabled (`Warning`)
/// - `2`: `FormAuth` enabled with no configured form users (`Warning`)
pub fn health_validate_form_auth(
    form_auth: bool,
    secure_control: bool,
    no_control_pass: bool,
    no_add_user: bool,
    no_restricted_user: bool,
) -> i32 {
    if !form_auth {
        return 0;
    }
    if !secure_control {
        return 1;
    }
    if no_control_pass && no_add_user && no_restricted_user {
        return 2;
    }
    0
}

/// Evaluates `ArticleCacheValidator::Validate`:
/// - `-1`: `val < 0` (`Error`)
/// - `1`: `val == 0` (`Warning` disabled)
/// - `-2`: 32-bit pointer (`ptr_size == 4`) and `val > 1900` (`Error`)
/// - `2`: `direct_write && val < 50` (`Warning` < 50MB)
/// - `3`: `!direct_write && val < 200` (`Warning` < 200MB)
/// - `0`: `Ok`
pub fn health_validate_article_cache(val: i32, ptr_size: usize, direct_write: bool) -> i32 {
    if val < 0 {
        return -1;
    }
    if val == 0 {
        return 1;
    }
    if ptr_size == 4 && val > 1900 {
        return -2;
    }
    if direct_write {
        if val < 50 {
            2
        } else {
            0
        }
    } else if val < 200 {
        3
    } else {
        0
    }
}

/// Evaluates `WriteBufferValidator::Validate`:
/// - `-1`: `val < 0` (`Error`)
/// - `1`: `val == 0` (`Warning`)
/// - `2`: `val < 1024` (`Warning`)
/// - `3`: `val > 102400` (`Warning`)
/// - `0`: `Ok`
pub fn health_validate_write_buffer(val: i32) -> i32 {
    if val < 0 {
        -1
    } else if val == 0 {
        1
    } else if val < 1024 {
        2
    } else if val > 102400 {
        3
    } else {
        0
    }
}

/// Evaluates `ServerPortValidator::Validate`:
/// - `0`: inactive or standard port (`Ok`)
/// - `-1`: invalid port (`< 1 || > 65535`) (`Error`)
/// - `1`: `tls` with non-standard port (`port != 443 && port != 563`) (`Info`)
/// - `2`: `!tls` with non-standard port (`port != 80 && port != 119`) (`Info`)
pub fn health_validate_server_port(active: bool, port: i32, tls: bool) -> i32 {
    if !active {
        return 0;
    }
    if !health_valid_port(port) {
        return -1;
    }
    if tls && port != 443 && port != 563 {
        return 1;
    }
    if !tls && port != 80 && port != 119 {
        return 2;
    }
    0
}

/// Evaluates `ServerConnectionsValidator::Validate`:
/// - `0`: inactive or `>= 8` (`Ok`)
/// - `1`: `max_conn == 0` (`Warning`)
/// - `-1`: `max_conn < 0 || max_conn > 999` (`Error`)
/// - `2`: `max_conn < 4 && dupe_search` (`Warning`)
/// - `3`: `max_conn < 8` (`Warning`)
pub fn health_validate_server_connections(active: bool, max_conn: i32, dupe_search: bool) -> i32 {
    if !active {
        return 0;
    }
    if max_conn == 0 {
        return 1;
    }
    if !(0..=999).contains(&max_conn) {
        return -1;
    }
    if max_conn < 4 && dupe_search {
        return 2;
    }
    if max_conn < 8 {
        return 3;
    }
    0
}

/// Evaluates `ServerRetentionValidator::Validate`:
/// - `0`: inactive or normal (`Ok`)
/// - `1`: `retention > 0 && retention < 100` (`Warning`)
/// - `2`: `retention > 10000` (`Info`)
pub fn health_validate_server_retention(active: bool, retention: i32) -> i32 {
    if !active {
        return 0;
    }
    if retention > 0 && retention < 100 {
        return 1;
    }
    if retention > 10000 {
        return 2;
    }
    0
}

/// Evaluates `ArticleReadChunkSizeValidator::Validate`, returning `(status_code, kb_val)`:
/// - `(-1, kb_val)`: `kb_val < 0` (`Error`)
/// - `(1, kb_val)`: `kb_val < 4` (`Warning`)
/// - `(2, kb_val)`: `kb_val > 10240` (`Info`)
/// - `(0, kb_val)`: `Ok`
pub fn health_validate_article_read_chunk_size(bytes: i32) -> (i32, i32) {
    let val = bytes / 1024;
    if val < 0 {
        (-1, val)
    } else if val < 4 {
        (1, val)
    } else if val > 10240 {
        (2, val)
    } else {
        (0, val)
    }
}

/// Computes the sorted index permutation for `ExtensionManager::Manager::Sort`.
/// Parses `,;` tokens from `order_str` (trimming ASCII control/whitespace `<= b' '`),
/// deduplicates tokens in first-seen order, places matching extension indices first
/// using the exact iterative swap order of `ExtensionManager::Manager::Sort`, and sorts
/// the remaining indices lexicographically by extension name.
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

/// Checks whether `path` is `/dev/null`, `/dev/stdout`, or `/dev/stderr`
/// (`SystemHealth::File::NullOrConsoleDevice`).
pub fn health_is_null_or_console_device(path: &[u8]) -> bool {
    matches!(path, b"/dev/null" | b"/dev/stdout" | b"/dev/stderr")
}

/// Extracts the inner IP slice from `hostname` when bracketed (`[ip]`), returning
/// `(has_brackets, ip_offset, ip_len)`.
pub fn health_extract_host_ip_slice(hostname: &[u8]) -> (bool, usize, usize) {
    if hostname.len() >= 2 && hostname.first() == Some(&b'[') && hostname.last() == Some(&b']') {
        (true, 1, hostname.len() - 2)
    } else {
        (false, 0, hostname.len())
    }
}

/// Validates RFC-like hostname syntax (labels separated by `.`, each 1..=63 bytes,
/// `[a-zA-Z0-9_]` at label ends and `[a-zA-Z0-9_-]` inside).
pub fn health_is_valid_hostname_syntax(hostname: &[u8]) -> bool {
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

/// Evaluates `SystemHealth::Network::ValidHost`, returning `(status_code, ip_offset, ip_len)`:
/// - `0`: Ok
/// - `1`: Error ("Hostname or IP address is empty")
/// - `2`: Error ("Hostname is too long (maximum 255 characters)")
/// - `3`: Error ("IPv6 address '<ip>' is provided, but connection mode is set to IPv4")
/// - `4`: Error ("IPv4 address '<ip>' is provided, but connection mode is set to IPv6")
/// - `5`: Error ("Remove brackets from the IP address, e.g., '<ip>'")
/// - `6`: Error ("Invalid hostname or IP address: '<hostname>'")
pub fn health_validate_host(
    hostname: &[u8],
    ipv: i32,
    ip_v4_mode: i32,
    ip_v6_mode: i32,
    is_v4: bool,
    is_v6: bool,
) -> (i32, usize, usize) {
    if hostname.is_empty() {
        return (1, 0, 0);
    }
    if hostname.len() > 255 {
        return (2, 0, 0);
    }
    let (has_brackets, ip_off, ip_len) = health_extract_host_ip_slice(hostname);
    if is_v4 || is_v6 {
        if ipv == ip_v4_mode && is_v6 {
            return (3, ip_off, ip_len);
        }
        if ipv == ip_v6_mode && is_v4 {
            return (4, ip_off, ip_len);
        }
        if has_brackets {
            return (5, ip_off, ip_len);
        }
        return (0, ip_off, ip_len);
    }
    if !has_brackets && health_is_valid_hostname_syntax(hostname) {
        return (0, ip_off, ip_len);
    }
    (6, ip_off, ip_len)
}

/// Evaluates `ControlUsernameValidator::Validate`:
/// - `1`: empty (`Info`)
/// - `2`: `"nzbget"` (`Info`)
/// - `0`: `Ok`
pub fn health_validate_control_username(username: &[u8]) -> i32 {
    if username.is_empty() {
        1
    } else if username == b"nzbget" {
        2
    } else {
        0
    }
}

/// Returns whether a secondary username (`AddUsername` or `RestrictedUsername`) is enabled
/// while `ControlUsername` is still set to the default `"nzbget"`.
pub fn health_is_default_control_user_with_subuser(sub_user: &[u8], control_user: &[u8]) -> bool {
    !sub_user.is_empty() && control_user == b"nzbget"
}

/// Evaluates `ArticleRetriesValidator::Validate`, returning `(status_code, warning_msg)`:
/// - `-1`: `val < 0 || val > 99` (`Error`)
/// - `1`: `val < 3` (`Warning` with formatted message in `warning_msg`)
/// - `0`: `Ok`
pub fn health_validate_article_retries(
    opt_name: &[u8],
    val: i32,
    hc_dupe: bool,
    dupe_search: bool,
) -> (i32, Vec<u8>) {
    if !(0..=99).contains(&val) {
        return (-1, Vec::new());
    }
    if val < 3 {
        let name_str = String::from_utf8_lossy(opt_name);
        let mut msg = format!(
            "'{name_str}' is set to {val}. This is very low; temporary connection drops may cause permanent download failures"
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
        return (1, msg.into_bytes());
    }
    (0, Vec::new())
}

/// Evaluates `ServerCipherValidator::Validate`:
/// - `0`: `Ok`
/// - `1`: `Warning` ("Cipher specified but TLS is disabled")
/// - `2`: `Info` ("Using custom cipher suite")
pub fn health_validate_server_cipher(active: bool, cipher: &[u8], tls: bool) -> i32 {
    if !active || cipher.is_empty() {
        0
    } else if !tls {
        1
    } else {
        2
    }
}

/// Evaluates `ServerIpVersionValidator::Validate`:
/// - `0`: `Ok` (`0` or `4`)
/// - `1`: `Info` (`6`)
/// - `-1`: `Error` (other)
pub fn health_validate_server_ip_version(ip_version: i32) -> i32 {
    match ip_version {
        0 | 4 => 0,
        6 => 1,
        _ => -1,
    }
}

/// Evaluates `FeedCertValidator::Validate`: returns `true` when `url` starts with
/// `"https://"` (case-insensitive) and `cert_none` is true.
pub fn health_validate_feed_cert(url: &[u8], cert_none: bool) -> bool {
    cert_none && url.len() >= 8 && url[..8].eq_ignore_ascii_case(b"https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontend_and_ncurses_helpers() {
        assert!(frontend_is_any_ip(b"0.0.0.0"));
        assert!(!frontend_is_any_ip(b"127.0.0.1"));

        assert_eq!(frontend_calc_log_start_index(10, 5), (6, false));
        assert_eq!(frontend_calc_log_start_index(2, 5), (0, true));

        assert_eq!(
            frontend_calc_remaining_hms(3661 * 1024, 1024, false, false),
            Some((1, 1, 1))
        );
        assert_eq!(frontend_calc_remaining_hms(1000, 100, true, false), None);
        assert_eq!(frontend_calc_remaining_hms(1000, 100, false, true), None);

        assert_eq!(
            ncurses_calc_window_sizes(24, 80, 512, 50, 3),
            (80, 4, 3, 4, 18, 17)
        );
        assert_eq!(
            ncurses_calc_window_sizes(24, 5, 512, 50, 0),
            (9, 2, 1, 2, 20, 19)
        );

        assert_eq!(ncurses_calc_scroll_and_selection(5, 10, 0, 4), (5, 3));
        assert_eq!(ncurses_calc_scroll_and_selection(-2, 10, 3, 4), (0, 0));

        assert_eq!(ncurses_calc_top_header_time(90061), (1, 1, 1, 1));

        assert_eq!(
            ncurses_resolve_group_pause_action(4, 100, 100, 2, false, 0, 0),
            (5, false)
        );
        assert_eq!(
            ncurses_resolve_group_pause_action(4, 100, 0, 2, false, 0, 0),
            (8, true)
        );
        assert_eq!(
            ncurses_resolve_group_pause_action(4, 100, 0, 2, true, 0, 0),
            (4, false)
        );

        assert_eq!(ncurses_step_rate_input(b'5' as i32, 8, 0, 0), (0, 1, 5));
        assert_eq!(ncurses_step_rate_input(b'0' as i32, 8, 1, 5), (0, 2, 50));
        assert_eq!(ncurses_step_rate_input(8, 8, 2, 50), (0, 1, 5));
        assert_eq!(ncurses_step_rate_input(13, 8, 1, 5), (1, 1, 5));
        assert_eq!(ncurses_step_rate_input(27, 8, 1, 5), (2, 1, 5));
    }

    #[test]
    fn test_maintenance_and_extension_helpers() {
        let json = br#"{"install-script": "install.sh", "update-info-script": "info.sh"}"#;
        let (start, len) =
            maintenance_extract_package_info_str(json, b"install-script", 1024).unwrap();
        assert_eq!(&json[start..start + len], b"install.sh");

        assert_eq!(maintenance_classify_update_message(b"normal line"), 0);
        assert_eq!(maintenance_classify_update_message(b"[NZB] QUIT"), 1);
        assert_eq!(maintenance_classify_update_message(b"[NZB] OTHER"), -1);

        assert_eq!(maintenance_classify_update_info_message(b"normal line"), 0);
        assert_eq!(
            maintenance_classify_update_info_message(b"[NZB] [UPDATEINFO]v1.0"),
            1
        );
        assert_eq!(maintenance_classify_update_info_message(b"[NZB] OTHER"), -1);

        let mut hex_line = b"0123456789abcdef".repeat(32);
        hex_line.extend_from_slice(b"\",");
        let mut sig = [0u8; 256];
        assert!(maintenance_decode_hex_signature(&hex_line, &mut sig));
        assert_eq!(sig[0], 0x01);
        assert_eq!(sig[7], 0xef);

        assert!(ext_should_skip_dir_entry(b".hidden"));
        assert!(ext_should_skip_dir_entry(b"_temp"));
        assert!(!ext_should_skip_dir_entry(b"Logger.py"));

        assert_eq!(ext_get_stem_len(b"Logger.py"), 6);
        assert_eq!(ext_get_stem_len(b"archive.tar.gz"), 11);
        assert_eq!(ext_get_stem_len(b"NoExt"), 5);

        assert!(ext_is_in_list(b"Foo, Bar ; Baz", b"Bar"));
        assert!(!ext_is_in_list(b"Foo, Bar ; Baz", b"Qux"));
    }

    #[test]
    fn test_nntp_and_systemhealth_helpers() {
        assert_eq!(health_check_password(b""), 1);
        assert_eq!(health_check_password(b"short"), 2);
        assert_eq!(health_check_password(b"longpassword"), 0);

        assert!(health_valid_port(6789));
        assert!(!health_valid_port(0));
        assert!(!health_valid_port(70000));

        assert_eq!(health_validate_control_ip(b"", false), -1);
        assert_eq!(health_validate_control_ip(b"0.0.0.0", false), 1);
        assert_eq!(health_validate_control_ip(b"/tmp/nzbget.sock", false), 2);
        assert_eq!(health_validate_control_ip(b"/tmp/nzbget.sock", true), -2);

        assert_eq!(health_validate_user_pass_pair(b"user", b""), 1);
        assert_eq!(health_validate_user_pass_pair(b"", b"pass"), 2);
        assert_eq!(health_validate_user_pass_pair(b"user", b"pass"), 0);

        assert_eq!(health_validate_form_auth(true, false, false, true, true), 1);
        assert_eq!(health_validate_form_auth(true, true, true, true, true), 2);
        assert_eq!(health_validate_form_auth(true, true, false, true, true), 0);

        assert_eq!(health_validate_article_cache(-1, 8, true), -1);
        assert_eq!(health_validate_article_cache(0, 8, true), 1);
        assert_eq!(health_validate_article_cache(2000, 4, true), -2);
        assert_eq!(health_validate_article_cache(30, 8, true), 2);
        assert_eq!(health_validate_article_cache(100, 8, false), 3);
        assert_eq!(health_validate_article_cache(250, 8, false), 0);

        assert_eq!(health_validate_write_buffer(-1), -1);
        assert_eq!(health_validate_write_buffer(0), 1);
        assert_eq!(health_validate_write_buffer(512), 2);
        assert_eq!(health_validate_write_buffer(200000), 3);
        assert_eq!(health_validate_write_buffer(4096), 0);

        assert_eq!(health_validate_server_port(true, 563, true), 0);
        assert_eq!(health_validate_server_port(true, 119, true), 1);
        assert_eq!(health_validate_server_port(true, 563, false), 2);

        assert_eq!(health_validate_server_connections(true, 0, false), 1);
        assert_eq!(health_validate_server_connections(true, 2, true), 2);
        assert_eq!(health_validate_server_connections(true, 4, true), 3);
        assert_eq!(health_validate_server_connections(true, 10, true), 0);

        assert_eq!(health_validate_server_retention(true, 50), 1);
        assert_eq!(health_validate_server_retention(true, 20000), 2);
        assert_eq!(health_validate_server_retention(true, 1000), 0);

        assert_eq!(health_validate_article_read_chunk_size(2048), (1, 2));
        assert_eq!(health_validate_article_read_chunk_size(65536), (0, 64));

        let names: [&[u8]; 4] = [b"zebra", b"Extension1", b"email", b"Extension2"];
        assert_eq!(
            ext_sort_indices(&names, b"Extension2, Extension1, email; Extension1; Extension1"),
            vec![3, 1, 2, 0]
        );
        assert_eq!(ext_sort_indices(&names, b""), vec![1, 3, 2, 0]);

        assert!(health_is_null_or_console_device(b"/dev/null"));
        assert!(health_is_null_or_console_device(b"/dev/stdout"));
        assert!(!health_is_null_or_console_device(b"/tmp/nzbget.log"));

        assert_eq!(health_extract_host_ip_slice(b"[::1]"), (true, 1, 3));
        assert_eq!(health_extract_host_ip_slice(b"news.example.com"), (false, 0, 16));
        assert!(health_is_valid_hostname_syntax(b"news.example-1.com"));
        assert!(!health_is_valid_hostname_syntax(b"-bad.example.com"));
        assert!(!health_is_valid_hostname_syntax(b"bad..example.com"));
        assert_eq!(health_validate_host(b"", 0, 1, 2, false, false).0, 1);
        assert_eq!(health_validate_host(b"::1", 1, 1, 2, false, true).0, 3);
        assert_eq!(health_validate_host(b"127.0.0.1", 2, 1, 2, true, false).0, 4);
        assert_eq!(health_validate_host(b"[127.0.0.1]", 0, 1, 2, true, false).0, 5);
        assert_eq!(health_validate_host(b"news.example.com", 0, 1, 2, false, false).0, 0);

        assert_eq!(health_validate_control_username(b""), 1);
        assert_eq!(health_validate_control_username(b"nzbget"), 2);
        assert_eq!(health_validate_control_username(b"admin"), 0);
        assert!(health_is_default_control_user_with_subuser(b"guest", b"nzbget"));
        assert!(!health_is_default_control_user_with_subuser(b"", b"nzbget"));

        let (code, msg) = health_validate_article_retries(b"ArticleRetries", 1, true, true);
        assert_eq!(code, 1);
        assert!(String::from_utf8_lossy(&msg).contains("HealthCheck=dupe and DupeSearch"));
        assert_eq!(health_validate_article_retries(b"ArticleRetries", 3, true, true).0, 0);
        assert_eq!(health_validate_article_retries(b"ArticleRetries", 100, false, false).0, -1);

        assert_eq!(health_validate_server_cipher(true, b"AES128", false), 1);
        assert_eq!(health_validate_server_cipher(true, b"AES128", true), 2);
        assert_eq!(health_validate_server_cipher(true, b"", false), 0);

        assert_eq!(health_validate_server_ip_version(0), 0);
        assert_eq!(health_validate_server_ip_version(6), 1);
        assert_eq!(health_validate_server_ip_version(9), -1);

        assert!(health_validate_feed_cert(b"HTTPS://indexer.example/rss", true));
        assert!(!health_validate_feed_cert(b"http://indexer.example/rss", true));
        assert!(!health_validate_feed_cert(b"https://indexer.example/rss", false));
    }
}
