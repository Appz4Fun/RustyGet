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

        let names: [&[u8]; 4] = [b"zebra", b"Extension1", b"email", b"Extension2"];
        assert_eq!(
            ext_sort_indices(&names, b"Extension2, Extension1, email; Extension1; Extension1"),
            vec![3, 1, 2, 0]
        );
        assert_eq!(ext_sort_indices(&names, b""), vec![1, 3, 2, 0]);
    }
}
