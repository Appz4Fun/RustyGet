//! DiskState, PrePostProcessor, Scanner, and TwinCheck hot-path routines.
//!
//! Ported from:
//! - `daemon/queue/DiskState.cpp`
//! - `daemon/postprocess/PrePostProcessor.cpp`
//! - `daemon/queue/Scanner.cpp`
//! - `daemon/queue/TwinCheck.cpp`

use std::os::raw::{c_char, c_int};

extern "C" {
    fn strtol(nptr: *const c_char, endptr: *mut *mut c_char, base: c_int) -> std::os::raw::c_long;
}

/// Convert a C string pointer safely to a byte slice.
pub unsafe fn cstr_to_bytes<'a>(ptr: *const c_char) -> &'a [u8] {
    if ptr.is_null() {
        return &[];
    }
    std::ffi::CStr::from_ptr(ptr).to_bytes()
}

#[inline]
fn ascii_lower(b: u8) -> u8 {
    if b.is_ascii_uppercase() {
        b + 32
    } else {
        b
    }
}

#[inline]
fn eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(&x, &y)| ascii_lower(x) == ascii_lower(y))
}

#[inline]
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Emulate C `atoi` on a byte slice.
pub fn c_atoi(bytes: &[u8]) -> i32 {
    let mut i = 0;
    while i < bytes.len() && is_c_space(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        return 0;
    }
    let mut neg = false;
    if bytes[i] == b'-' {
        neg = true;
        i += 1;
    } else if bytes[i] == b'+' {
        i += 1;
    }
    let mut acc: i64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        acc = acc.wrapping_mul(10).wrapping_add((bytes[i] - b'0') as i64);
        i += 1;
    }
    if neg {
        (-acc) as i32
    } else {
        acc as i32
    }
}

/// Parse `%i` style integer (base 10, `0x` hex, or `0` octal) like `sscanf("%i")` / `strtol(..., 0)`.
fn parse_scanf_i32(bytes: &[u8]) -> Option<(i32, usize)> {
    let mut i = 0;
    while i < bytes.len() && is_c_space(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let mut cbuf = Vec::with_capacity(bytes.len() - i + 1);
    cbuf.extend_from_slice(&bytes[i..]);
    cbuf.push(0);
    let start_ptr = cbuf.as_ptr() as *const c_char;
    let mut end_ptr: *mut c_char = std::ptr::null_mut();
    let val = unsafe { strtol(start_ptr, &mut end_ptr, 0) };
    let consumed = unsafe { end_ptr.offset_from(start_ptr) as usize };
    if consumed == 0 {
        return None;
    }
    Some((val as i32, i + consumed))
}

// ============================================================================
// 1. DiskState (`daemon/queue/DiskState.cpp`)
// ============================================================================

const FORMATVERSION_SIGNATURE: &[u8] = b"nzbget diskstate file version ";

/// `StateFile::ParseFormatVersion(const char* formatSignature)`
pub fn parse_format_version(format_signature: &[u8]) -> i32 {
    if !format_signature.starts_with(FORMATVERSION_SIGNATURE) {
        return 0;
    }
    c_atoi(&format_signature[FORMATVERSION_SIGNATURE.len()..])
}

/// `StateDiskFile::PrintLine` line-break sanitization: replaces `\n` and `\r` with `' '`.
pub fn sanitize_line_breaks_in_place(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        if *b == b'\n' || *b == b'\r' {
            *b = b' ';
        }
    }
}

/// `DiskState::RaiseIdsPastQueueDir` `"nzb %i file %i"` parser.
pub fn parse_ids_file(text: &[u8]) -> Option<(i32, i32)> {
    if !text.starts_with(b"nzb") {
        return None;
    }
    let rest = &text[3..];
    if rest.is_empty() || !is_c_space(rest[0]) {
        return None;
    }
    let (nzb_id, used1) = parse_scanf_i32(rest)?;
    let mut after1 = &rest[used1..];
    if after1.is_empty() || !is_c_space(after1[0]) {
        return None;
    }
    while !after1.is_empty() && is_c_space(after1[0]) {
        after1 = &after1[1..];
    }
    if !after1.starts_with(b"file") {
        return None;
    }
    let rest2 = &after1[4..];
    if rest2.is_empty() || !is_c_space(rest2[0]) {
        return None;
    }
    let (file_id, _) = parse_scanf_i32(rest2)?;
    if nzb_id >= 0 && file_id >= 0 {
        Some((nzb_id, file_id))
    } else {
        None
    }
}

/// `DiskState::RaiseIdsPastQueueDir` filename classifier.
/// Returns `Some((id, is_nzb_log))` when the filename is `"n<id>.log"` or `"<id>"` / `"<id>s"` / `"<id>c"`.
pub fn classify_queue_dir_id(filename: &[u8]) -> Option<(i32, bool)> {
    if filename.is_empty() {
        return None;
    }
    let nzb_log = filename[0] == b'n';
    let p = if nzb_log { &filename[1..] } else { filename };
    if p.is_empty() || !p[0].is_ascii_digit() {
        return None;
    }
    let mut num_end = 0;
    let mut id: i64 = 0;
    while num_end < p.len() && p[num_end].is_ascii_digit() {
        id = id.saturating_mul(10).saturating_add((p[num_end] - b'0') as i64);
        num_end += 1;
    }
    if id <= 0 || id > i32::MAX as i64 {
        return None;
    }
    let suffix = &p[num_end..];
    if nzb_log && suffix == b".log" {
        Some((id as i32, true))
    } else if !nzb_log && (suffix.is_empty() || suffix == b"s" || suffix == b"c") {
        Some((id as i32, false))
    } else {
        None
    }
}

/// `DiskState::ConvertLegacyFileStates` candidate classifier.
/// `suffix_kind`: `0` = `""`, `1` = `"s"`, `2` = `"c"`.
pub fn classify_legacy_file_state(
    filename: &[u8],
    header_line: &[u8],
    file_version: i32,
    legacy_version: i32,
) -> Option<(i32, i32)> {
    let mut num_end = 0;
    while num_end < filename.len() && filename[num_end].is_ascii_digit() {
        num_end += 1;
    }
    if num_end == 0 {
        return None;
    }
    let suffix = &filename[num_end..];
    let suffix_kind = if suffix.is_empty() {
        0
    } else if suffix == b"s" {
        1
    } else if suffix == b"c" {
        2
    } else {
        return None;
    };

    if !header_line.starts_with(FORMATVERSION_SIGNATURE) {
        return None;
    }
    let (version, _) = parse_scanf_i32(&header_line[FORMATVERSION_SIGNATURE.len()..])?;
    if version > file_version && version <= legacy_version {
        Some((c_atoi(filename), suffix_kind))
    } else {
        None
    }
}

#[inline]
pub fn join_i64(high: u32, low: u32) -> i64 {
    (((high as u64) << 32) | (low as u64)) as i64
}

/// `DiskState::LoadDupeExtras` hole offset/size validator.
pub fn validate_repair_hole(
    offset_hi: u32,
    offset_lo: u32,
    size_hi: u32,
    size_lo: u32,
    decoded_size: i64,
) -> Option<(i64, i64)> {
    let offset = join_i64(offset_hi, offset_lo);
    let size = join_i64(size_hi, size_lo);
    if offset < 0 || size <= 0 || decoded_size <= 0 || offset > decoded_size || size > decoded_size - offset {
        None
    } else {
        Some((offset, size))
    }
}

/// `DiskState::SaveDupeTrailer` predicate for whether an `NzbInfo` has dupe extras.
pub fn has_dupe_extras(
    recovered_articles: i32,
    recovered_bytes: i64,
    recovered_holes: i32,
    repair_jobs_count: usize,
) -> bool {
    recovered_articles != 0 || recovered_bytes != 0 || recovered_holes != 0 || repair_jobs_count > 0
}

/// `DiskState::SaveDupeTrailer` `UnpackTried` bitmask packer.
pub fn pack_unpack_tried_flags(unpack_tried: bool, par_repaired: bool) -> i32 {
    (if unpack_tried { 1 } else { 0 }) | (if par_repaired { 2 } else { 0 })
}

fn parse_prefixed_or_plain_int(line: &[u8], prefix: &[u8]) -> Option<i32> {
    if line.starts_with(prefix) {
        parse_scanf_i32(&line[prefix.len()..]).map(|(v, _)| v)
    } else {
        parse_scanf_i32(line).map(|(v, _)| v)
    }
}

/// `DiskState::LoadDupeTrailer` / `LoadNzbInfo` unpack state line parser.
pub fn parse_unpack_state_lines(
    line1: &[u8],
    line2: &[u8],
    line3: &[u8],
    ns_running: i32,
    ns_none: i32,
) -> Option<(i32, bool, bool, i32)> {
    let mut direct_status = parse_prefixed_or_plain_int(line1, b"DirectUnpackStatus=")?;
    if direct_status == ns_running {
        direct_status = ns_none;
    }
    let unpack_tried_flags = parse_prefixed_or_plain_int(line2, b"UnpackTried=")?;
    let unpack_tried = (unpack_tried_flags & 1) != 0;
    let par_repaired = (unpack_tried_flags & 2) != 0;
    let last_unpack_status = parse_prefixed_or_plain_int(line3, b"LastUnpackStatus=")?;
    Some((direct_status, unpack_tried, par_repaired, last_unpack_status))
}

/// `DiskState::LoadNzbInfo` post-stage migration and transient stage clamp.
pub fn normalize_post_stage(mut post_stage: i32, format_version: i32, pt_finished: i32) -> i32 {
    if format_version < 59 && post_stage == 6 {
        post_stage += 1;
    } else if format_version < 59 && post_stage > 6 {
        post_stage += 2;
    }
    if post_stage > pt_finished {
        post_stage = pt_finished;
    }
    post_stage
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsArticleLoadEval {
    pub valid: i32,
    pub status: i32,
    pub segment_offset: i64,
    pub segment_size: i32,
    pub dupe_fallback_round: i32,
    pub need_result_filename: i32,
    pub completed_articles: i32,
}

/// `DiskState::LoadFileState` per-article validation and status normalization.
#[allow(clippy::too_many_arguments)]
pub fn eval_article_load_state(
    format_version: i32,
    completed: bool,
    fields: i32,
    status_in: i32,
    segment_offset_in: i64,
    segment_size_in: i32,
    staged_fallback_in: i32,
    part_number: i32,
    staged_file_exists: bool,
    direct_write: bool,
    force_direct_write: bool,
    has_result_filename: bool,
    completed_articles_in: i32,
    total_size: i32,
) -> RsArticleLoadEval {
    const AI_UNDEFINED: i32 = 0;
    const AI_RUNNING: i32 = 1;
    const AI_FINISHED: i32 = 2;

    let mut status = status_in;
    let mut segment_offset = segment_offset_in;
    let mut segment_size = segment_size_in;
    let mut dupe_fallback_round = 0;

    if format_version >= 2 {
        let mut staged_fallback = staged_fallback_in;
        if (if format_version >= 9 { fields != 5 } else { fields < 4 })
            || (staged_fallback != 0 && staged_fallback != 1)
        {
            return RsArticleLoadEval {
                valid: 0,
                ..Default::default()
            };
        }
        if !completed && (segment_offset < 0 || segment_size < 0 || segment_size > 1024 * 1024 * 1024) {
            status = AI_UNDEFINED;
            staged_fallback = 0;
            segment_offset = 0;
            segment_size = 0;
        }
        if staged_fallback != 0 && part_number > 0 && staged_file_exists {
            status = AI_FINISHED;
        } else {
            staged_fallback = 0;
        }
        dupe_fallback_round = if !completed && status == AI_FINISHED && staged_fallback != 0 {
            1
        } else {
            0
        };
    } else if fields != 1 {
        return RsArticleLoadEval {
            valid: 0,
            ..Default::default()
        };
    }

    if status == AI_RUNNING {
        status = AI_UNDEFINED;
    }

    if !completed
        && format_version < 9
        && status == AI_FINISHED
        && (direct_write || force_direct_write)
        && part_number > 0
        && staged_file_exists
    {
        dupe_fallback_round = 1;
    }

    let need_result_filename = status == AI_FINISHED
        && ((!direct_write && !force_direct_write) || dupe_fallback_round > 0)
        && !has_result_filename;

    if completed_articles_in == total_size - 1 && !completed {
        status = AI_UNDEFINED;
        dupe_fallback_round = 0;
    }

    let completed_articles = if status != AI_UNDEFINED {
        completed_articles_in + 1
    } else {
        completed_articles_in
    };

    RsArticleLoadEval {
        valid: 1,
        status,
        segment_offset,
        segment_size,
        dupe_fallback_round,
        need_result_filename: if need_result_filename { 1 } else { 0 },
        completed_articles,
    }
}

/// `DiskState::LoadFileState` DirectWrite `OutputInitialized` reconciliation.
/// Returns `(new_output_initialized, pick_path_kind)` where `pick_path_kind` is:
/// - `0`: keep current `OutputFilename`
/// - `1`: set `OutputFilename` to `<destDir>/<id>.out.tmp`
/// - `2`: set `OutputFilename` to `<destDir>/<filename>`
pub fn reconcile_direct_write_output(
    output_initialized: bool,
    has_own_out_file: bool,
    has_out_tmp: bool,
    has_completed_dest: bool,
    success_articles: i32,
    completed: bool,
    output_filename_empty: bool,
) -> (bool, i32) {
    let has_direct_write_output = has_own_out_file || has_out_tmp || has_completed_dest;
    let mut new_init = output_initialized;
    if new_init && !has_direct_write_output {
        new_init = false;
    } else if !new_init && has_direct_write_output && (success_articles > 0 || completed) {
        new_init = true;
    }

    let pick_path_kind = if new_init && output_filename_empty && (has_out_tmp || has_completed_dest) {
        if has_out_tmp {
            1
        } else {
            2
        }
    } else {
        0
    };

    (new_init, pick_path_kind)
}

/// `DiskState::CleanupTempDir` filename classifier.
/// Returns:
/// - `(1, 0, 0)` if immediate garbage (`.tmp` or `.dec` substring)
/// - `(2, id, part)` if matches `%i.%i` (caller checks whether `id` is in queue)
/// - `(0, 0, 0)` otherwise
pub fn classify_temp_file(filename: &[u8]) -> (i32, i32, i32) {
    if filename.windows(4).any(|w| w == b".tmp" || w == b".dec") {
        return (1, 0, 0);
    }
    if let Some((id, used1)) = parse_scanf_i32(filename) {
        let rest = &filename[used1..];
        if rest.starts_with(b".") {
            let after_dot = &rest[1..];
            if !after_dot.is_empty() && !is_c_space(after_dot[0]) {
                if let Some((part, _)) = parse_scanf_i32(after_dot) {
                    return (2, id, part);
                }
            }
        }
    }
    (0, 0, 0)
}

/// `DiskState::CleanupQueueDir` orphan file detector.
pub fn is_orphan_queue_file(
    filename: &[u8],
    sorted_file_ids: &[i32],
    sorted_nzb_ids: &[i32],
) -> bool {
    let mut del = false;

    if let Some((id, used)) = parse_scanf_i32(filename) {
        let rest = &filename[used..];
        let is_state_suffix = !rest.is_empty() && (rest[0] == b's' || rest[0] == b'c');
        let is_plain_id = !filename.contains(&b'.');
        if is_state_suffix || is_plain_id {
            del = sorted_file_ids.binary_search(&id).is_err();
        }
    }

    if !del && filename.starts_with(b"n") {
        let after_n = &filename[1..];
        if !after_n.is_empty() && !is_c_space(after_n[0]) {
            if let Some((id, used)) = parse_scanf_i32(after_n) {
                if after_n[used..].starts_with(b".log") {
                    del = sorted_nzb_ids.binary_search(&id).is_err();
                }
            }
        }
    }

    del
}

#[derive(Debug, Clone)]
pub struct ServerMatchInput<'a> {
    pub name: &'a [u8],
    pub host: &'a [u8],
    pub port: i32,
    pub user: &'a [u8],
}

#[derive(Debug, Clone)]
pub struct ServerRefEntry<'a> {
    pub state_id: i32,
    pub name: &'a [u8],
    pub host: &'a [u8],
    pub port: i32,
    pub user: &'a [u8],
    pub matched: bool,
    pub perfect: bool,
}

#[derive(Clone, Copy)]
enum MatchCriteria {
    Name,
    Host,
    Port,
    User,
}

fn find_candidates(
    server: &ServerMatchInput<'_>,
    refs: &[ServerRefEntry<'_>],
    candidates: &mut Vec<usize>,
    criteria: MatchCriteria,
    keep_if_nothing: bool,
) {
    let orig = candidates.clone();
    candidates.retain(|&idx| {
        let r = &refs[idx];
        let m = match criteria {
            MatchCriteria::Name => eq_ignore_ascii_case(server.name, r.name),
            MatchCriteria::Host => eq_ignore_ascii_case(server.host, r.host),
            MatchCriteria::Port => server.port == r.port,
            MatchCriteria::User => eq_ignore_ascii_case(server.user, r.user),
        };
        m && !r.matched
    });
    if candidates.is_empty() && keep_if_nothing {
        *candidates = orig;
    }
}

/// `DiskState::MatchServers` 2-pass server state ID matcher.
/// Populates `out_server_state_ids` (length `servers.len()`) and returns `perfect_match`.
pub fn match_servers(
    servers: &[ServerMatchInput<'_>],
    refs: &mut [ServerRefEntry<'_>],
    out_server_state_ids: &mut [i32],
) -> bool {
    for slot in out_server_state_ids.iter_mut() {
        *slot = 0;
    }

    // Step 1: trying perfect match
    for (s_idx, server) in servers.iter().enumerate() {
        let mut matched_refs: Vec<usize> = (0..refs.len()).collect();
        find_candidates(server, refs, &mut matched_refs, MatchCriteria::Name, false);
        find_candidates(server, refs, &mut matched_refs, MatchCriteria::Host, false);
        find_candidates(server, refs, &mut matched_refs, MatchCriteria::Port, false);
        find_candidates(server, refs, &mut matched_refs, MatchCriteria::User, false);

        if matched_refs.len() == 1 {
            let r_idx = matched_refs[0];
            out_server_state_ids[s_idx] = refs[r_idx].state_id;
            refs[r_idx].matched = true;
            refs[r_idx].perfect = true;
        }
    }

    // Step 2: matching host, port, username and server-name
    for (s_idx, server) in servers.iter().enumerate() {
        if out_server_state_ids[s_idx] == 0 {
            let mut matched_refs: Vec<usize> = (0..refs.len()).collect();
            find_candidates(server, refs, &mut matched_refs, MatchCriteria::Host, false);

            if matched_refs.len() > 1 {
                find_candidates(server, refs, &mut matched_refs, MatchCriteria::Name, true);
            }
            if matched_refs.len() > 1 {
                find_candidates(server, refs, &mut matched_refs, MatchCriteria::User, true);
            }
            if matched_refs.len() > 1 {
                find_candidates(server, refs, &mut matched_refs, MatchCriteria::Port, true);
            }

            if !matched_refs.is_empty() {
                let r_idx = matched_refs[0];
                out_server_state_ids[s_idx] = refs[r_idx].state_id;
                refs[r_idx].matched = true;
            }
        }
    }

    let all_refs_perfect = refs.iter().all(|r| r.perfect);
    let all_servers_matched = out_server_state_ids.iter().all(|&id| id != 0);
    all_refs_perfect && all_servers_matched
}

/// `DiskState::AppendNzbMessage` bad-character sanitizer (`\n`, `\r`, `\t` -> `' '`).
pub fn sanitize_log_text_in_place(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        if *b == b'\n' || *b == b'\r' || *b == b'\t' {
            *b = b' ';
        }
    }
}

/// `DiskState::LoadNzbMessages` log line parser.
/// Returns `Some((time, kind, text_offset))` where `kind` is `Message::EKind`
/// (`mkInfo = 0, mkWarning = 1, mkError = 2, mkDebug = 3, mkDetail = 4`).
pub fn parse_log_line(line: &[u8]) -> Option<(i32, i32, usize)> {
    let tab1 = line.iter().position(|&b| b == b'\t')?;
    let after1 = &line[tab1 + 1..];
    let time = c_atoi(after1);

    let tab2_rel = after1.iter().position(|&b| b == b'\t')?;
    let kind_slice = &after1[tab2_rel + 1..];
    let kind = if kind_slice.starts_with(b"INFO") {
        0 // mkInfo
    } else if kind_slice.starts_with(b"WARNING") {
        1 // mkWarning
    } else if kind_slice.starts_with(b"ERROR") {
        2 // mkError
    } else if kind_slice.starts_with(b"DETAIL") {
        4 // mkDetail
    } else if kind_slice.starts_with(b"DEBUG") {
        3 // mkDebug
    } else {
        2 // mkError
    };

    let tab3_rel = kind_slice.iter().position(|&b| b == b'\t')?;
    let text_offset = (tab1 + 1) + (tab2_rel + 1) + (tab3_rel + 1);
    Some((time, kind, text_offset))
}

// ============================================================================
// 2. PrePostProcessor (`daemon/postprocess/PrePostProcessor.cpp`)
// ============================================================================

/// `PrePostProcessor::SanitisePostQueue` stage & `RequestParCheck` transition.
pub fn sanitise_post_stage(
    stage: i32,
    pt_executing_script: i32,
    pt_finished: i32,
    pt_queued: i32,
    dest_dir_exists: bool,
    unpack_tried: bool,
    par_status: i32,
    ps_skipped: i32,
) -> (i32, bool) {
    if stage == pt_executing_script || !dest_dir_exists {
        (pt_finished, false)
    } else {
        let request_par = unpack_tried && par_status <= ps_skipped;
        (pt_queued, request_par)
    }
}

/// `PrePostProcessor::DownloadQueueUpdate` `eaFileCompleted` / `eaFileDeleted` classifier.
/// Returns:
/// - `1`: collection completely downloaded (`qeNzbDownloaded` + `NzbDownloaded`)
/// - `2`: collection deleted from queue (`NzbDeleted`)
/// - `0`: no collection transition
#[allow(clippy::too_many_arguments)]
pub fn classify_file_event(
    is_file_completed: bool,
    is_file_deleted: bool,
    dupe_deleted: bool,
    delete_status: i32,
    ds_none: i32,
    ds_health: i32,
    has_post_info: bool,
    download_completed_ignore_paused: bool,
    download_completed_all: bool,
) -> i32 {
    if (is_file_completed || dupe_deleted)
        && delete_status != ds_health
        && !has_post_info
        && download_completed_ignore_paused
    {
        1
    } else if (is_file_deleted || (is_file_completed && delete_status > ds_none))
        && !has_post_info
        && download_completed_all
    {
        2
    } else {
        0
    }
}

/// `PrePostProcessor::NzbCompleted` action flags evaluator.
/// Returns `(add_to_history, call_dupe_completed, enqueue_deleted_script, need_save)`.
#[allow(clippy::too_many_arguments)]
pub fn eval_nzb_completed(
    dupe_hint: i32,
    dh_redownload_auto: i32,
    keep_history: i32,
    avoid_history: bool,
    dupe_check: bool,
    dupe_mode: i32,
    dm_force: i32,
    fails_over: bool,
    delete_status: i32,
    ds_none: i32,
    ds_health: i32,
    ds_bad: i32,
    ds_scan: i32,
    ds_copy: i32,
) -> (bool, bool, bool, bool) {
    let download_dupe = dupe_hint == dh_redownload_auto;
    let add_to_history = keep_history > 0 && !avoid_history;
    let call_dupe_completed = dupe_check
        && (dupe_mode != dm_force || fails_over)
        && (delete_status == ds_none
            || delete_status == ds_health
            || delete_status == ds_bad
            || delete_status == ds_scan
            || (delete_status == ds_copy && download_dupe));
    let enqueue_deleted_script =
        delete_status > ds_none && delete_status != ds_health && delete_status != ds_bad;
    let need_save = true;
    (
        add_to_history,
        call_dupe_completed,
        enqueue_deleted_script,
        need_save,
    )
}

/// `PrePostProcessor::CanRunMoreJobs` scheduler evaluator.
/// `post_strategy`: `0` = `ppSequential`, `1` = `ppBalanced`, `2` = `ppAggressive`, `3` = `ppRocket`.
/// Returns `(can_run, allow_par)`.
pub fn can_run_more_jobs(
    job_stages: &[i32],
    pt_loading_pars: i32,
    pt_verifying_repaired: i32,
    pt_repairing: i32,
    post_strategy: i32,
) -> (bool, bool) {
    let total_jobs = job_stages.len() as i32;
    let mut par_jobs = 0;
    let mut other_jobs = 0;
    let mut repair_jobs = false;

    for &stage in job_stages {
        let par_job = stage >= pt_loading_pars && stage <= pt_verifying_repaired;
        repair_jobs |= stage == pt_repairing;
        if par_job {
            par_jobs += 1;
        } else {
            other_jobs += 1;
        }
    }

    match post_strategy {
        0 => (total_jobs == 0, true), // ppSequential
        1 => (
            other_jobs == 0 && (par_jobs == 0 || repair_jobs),
            par_jobs == 0,
        ), // ppBalanced
        2 => (total_jobs < 3, par_jobs < 1), // ppAggressive
        3 => (total_jobs < 6, par_jobs < 2), // ppRocket
        _ => (false, false),
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RsPostJobCandidate {
    pub has_post_info: u8,
    pub working: u8,
    pub has_queue_script_job: u8,
    pub direct_unpack_running: u8,
    pub has_live_repair_thread: u8,
    pub priority: i32,
    pub force_priority: u8,
    pub need_par_check: u8,
    pub need_unpack: u8,
    pub in_active_jobs: u8,
    pub download_completed: u8,
}

/// `PrePostProcessor::PickNextJob` candidate selector.
/// Returns the selected index or `-1`.
pub fn pick_next_post_job(
    candidates: &[RsPostJobCandidate],
    allow_par: bool,
    has_active_unpack: bool,
    pause_post_process: bool,
) -> i32 {
    let mut best_idx: i32 = -1;
    let mut best_priority: i32 = i32::MIN;

    for (idx, c) in candidates.iter().enumerate() {
        if c.has_post_info != 0
            && c.working == 0
            && c.has_queue_script_job == 0
            && c.direct_unpack_running == 0
            && c.has_live_repair_thread == 0
            && (best_idx < 0 || c.priority > best_priority)
            && (!pause_post_process || c.force_priority != 0)
            && (allow_par || c.need_par_check == 0)
            && (!has_active_unpack || c.need_unpack == 0)
            && c.in_active_jobs == 0
            && c.download_completed != 0
        {
            best_idx = idx as i32;
            best_priority = c.priority;
        }
    }

    best_idx
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsPostStageDecision {
    pub unpack: i32,
    pub skip_unpack_due_to_par: i32,
    pub cleanup: i32,
    pub move_inter: i32,
    pub post_download_renaming: i32,
    pub post_unpack_renaming: i32,
}

/// `PrePostProcessor::StartJob` stage eligibility calculator (lines 1166–1261).
#[allow(clippy::too_many_arguments)]
pub fn eval_post_stages(
    want_unpack: bool,
    unpack_status: i32,
    delete_status: i32,
    par_status: i32,
    cleanup_status: i32,
    has_ext_cleanup_disk: bool,
    health: i32,
    move_status: i32,
    in_inter_dir: bool,
    direct_unpack_status: i32,
    opt_post_download_rename: bool,
    post_download_renaming_status: i32,
    has_dest_dir: bool,
    has_name: bool,
    opt_rename_after_unpack: bool,
    post_unpack_renaming_status: i32,
) -> RsPostStageDecision {
    // NzbInfo enum constants:
    // EDeleteStatus: dsNone = 0
    // EParStatus: psNone = 0, psSkipped = 1, psFailure = 2, psSuccess = 3, psRepairPossible = 4, psManual = 5
    // EPostUnpackStatus: usNone = 0, usSkipped = 1, usFailure = 2, usSuccess = 3, usSpace = 4, usPassword = 5
    // ECleanupStatus: csNone = 0
    // EMoveStatus: msNone = 0, msFailure = 1, msSuccess = 2
    // EDirectUnpackStatus: nsNone = 0, nsRunning = 1, nsFailure = 2, nsSuccess = 3
    const DS_NONE: i32 = 0;
    const PS_NONE: i32 = 0;
    const PS_SKIPPED: i32 = 1;
    const PS_FAILURE: i32 = 2;
    const PS_SUCCESS: i32 = 3;
    const PS_REPAIR_POSSIBLE: i32 = 4;
    const PS_MANUAL: i32 = 5;
    const US_NONE: i32 = 0;
    const US_SKIPPED: i32 = 1;
    const US_FAILURE: i32 = 2;
    const US_SUCCESS: i32 = 3;
    const US_SPACE: i32 = 4;
    const US_PASSWORD: i32 = 5;
    const CS_NONE: i32 = 0;
    const MS_NONE: i32 = 0;
    const MS_SUCCESS: i32 = 2;
    const NS_SUCCESS: i32 = 3;

    let mut unpack = want_unpack && unpack_status == US_NONE && delete_status == DS_NONE;
    let par_failed =
        par_status == PS_FAILURE || par_status == PS_REPAIR_POSSIBLE || par_status == PS_MANUAL;

    let cleanup = !unpack
        && want_unpack
        && cleanup_status == CS_NONE
        && has_ext_cleanup_disk
        && ((par_status == PS_SUCCESS
            && unpack_status != US_FAILURE
            && unpack_status != US_SPACE
            && unpack_status != US_PASSWORD)
            || (unpack_status == US_SUCCESS && par_status != PS_FAILURE)
            || ((unpack_status == US_NONE || unpack_status == US_SKIPPED)
                && (par_status == PS_NONE || par_status == PS_SKIPPED)
                && health == 1000));

    let move_inter = !unpack
        && move_status == MS_NONE
        && unpack_status != US_FAILURE
        && unpack_status != US_SPACE
        && unpack_status != US_PASSWORD
        && par_status != PS_FAILURE
        && par_status != PS_MANUAL
        && delete_status == DS_NONE
        && !((unpack_status == US_NONE || unpack_status == US_SKIPPED)
            && (par_status == PS_NONE || par_status == PS_SKIPPED)
            && health < 1000)
        && in_inter_dir;

    let mut skip_unpack_due_to_par = false;
    let mut effective_unpack_status = unpack_status;
    if unpack && par_failed && direct_unpack_status != NS_SUCCESS {
        skip_unpack_due_to_par = true;
        effective_unpack_status = US_SKIPPED;
        unpack = false;
    }

    let post_download_renaming = opt_post_download_rename
        && post_download_renaming_status == 0
        && has_dest_dir
        && has_name
        && (effective_unpack_status == US_NONE || effective_unpack_status == US_SKIPPED)
        && effective_unpack_status != US_FAILURE
        && effective_unpack_status != US_SPACE
        && effective_unpack_status != US_PASSWORD
        && par_status != PS_FAILURE
        && par_status != PS_MANUAL
        && !move_inter
        && (move_status == MS_SUCCESS || move_status == MS_NONE);

    let post_unpack_renaming = opt_rename_after_unpack
        && post_unpack_renaming_status == 0
        && has_dest_dir
        && has_name
        && effective_unpack_status == US_SUCCESS
        && par_status != PS_FAILURE
        && par_status != PS_MANUAL
        && !move_inter
        && (move_status == MS_SUCCESS || move_status == MS_NONE);

    RsPostStageDecision {
        unpack: if unpack { 1 } else { 0 },
        skip_unpack_due_to_par: if skip_unpack_due_to_par { 1 } else { 0 },
        cleanup: if cleanup { 1 } else { 0 },
        move_inter: if move_inter { 1 } else { 0 },
        post_download_renaming: if post_download_renaming { 1 } else { 0 },
        post_unpack_renaming: if post_unpack_renaming { 1 } else { 0 },
    }
}

/// `PrePostProcessor::UpdatePauseState` pause-download evaluator.
pub fn calc_post_pause_state(
    job_stages: &[i32],
    par_pause_queue: bool,
    unpack_pause_queue: bool,
    script_pause_queue: bool,
) -> bool {
    // PostInfo::EStage:
    // ptQueued = 0, ptLoadingPars = 1, ptVerifyingSources = 2, ptRepairing = 3,
    // ptVerifyingRepaired = 4, ptParRenaming = 5, ptRarRenaming = 6, ptUnpacking = 7,
    // ptCleaningUp = 8, ptMoving = 9, ptPostUnpackRenaming = 10, ptExecutingScript = 11,
    // ptFinished = 12, ptPostDownloadRenaming = 13, ptStreamRepairing = 14
    let mut need_pause = false;
    for &stage in job_stages {
        match stage {
            1..=5 | 14 => need_pause |= par_pause_queue,
            6..=10 | 13 => need_pause |= unpack_pause_queue,
            11 => need_pause |= script_pause_queue,
            _ => {}
        }
    }
    need_pause
}

// ============================================================================
// 3. Scanner (`daemon/queue/Scanner.cpp`)
// ============================================================================

/// `Scanner::ServiceWork` multi-pass interval & pass counter update.
pub fn scanner_next_pass(
    nzb_dir_interval: i32,
    nzb_dir_file_age: i32,
    has_scan_script: bool,
    current_pass: i32,
) -> (i32, i32) {
    let mut next_interval = nzb_dir_interval;
    let mut next_pass = current_pass;
    if nzb_dir_interval > 0 && nzb_dir_file_age < nzb_dir_interval {
        let max_pass = if has_scan_script { 3 } else { 1 };
        if current_pass < max_pass {
            next_interval = nzb_dir_file_age;
            next_pass = current_pass + 1;
        } else {
            next_pass = 0;
        }
    }
    (next_interval, next_pass)
}

/// `Scanner::CanProcessFile` extension filter.
pub fn can_process_extension(full_filename: &[u8]) -> bool {
    let dot_pos = match full_filename.iter().rposition(|&b| b == b'.') {
        Some(pos) => pos,
        None => return false,
    };
    let ext = &full_filename[dot_pos..];
    !eq_ignore_ascii_case(ext, b".queued")
        && !eq_ignore_ascii_case(ext, b".error")
        && !eq_ignore_ascii_case(ext, b".processed")
}

/// `Scanner::CanProcessFile` file size & age stabilization check.
/// Returns `(can_process, updated_size, updated_last_change)`.
pub fn eval_file_age(
    old_size: i64,
    new_size: i64,
    last_change: i64,
    current_time: i64,
    file_age: i32,
) -> (bool, i64, i64) {
    if old_size == new_size && current_time - last_change >= file_age as i64 {
        (true, old_size, last_change)
    } else if old_size != new_size {
        (false, new_size, current_time)
    } else {
        (false, old_size, last_change)
    }
}

/// `Scanner::DropOldFiles` stale entry predicate.
pub fn should_drop_scan_file(
    last_change: i64,
    current_time: i64,
    nzb_dir_interval: i32,
    nzb_dir_file_age: i32,
) -> bool {
    let threshold = ((nzb_dir_interval as i64) + (nzb_dir_file_age as i64)) * 2;
    (current_time - last_change >= threshold) || current_time < last_change
}

/// `Scanner::AddFileToQueue` missed-articles severity classifier.
/// Returns `0` (none), `1` (`Message::mkDetail`), `2` (`Message::mkWarning`).
pub fn classify_missed_articles(
    missed_articles: i32,
    total_articles: i32,
    is_cut_short: bool,
) -> i32 {
    if missed_articles <= 0 {
        return 0;
    }
    let large = missed_articles >= 100 || missed_articles.saturating_mul(10) >= total_articles;
    if is_cut_short || large {
        2
    } else {
        1
    }
}

/// `Scanner::AddExternalFile` XML/NZB header detector (matches `BString<1024>` truncation).
pub fn is_nzb_xml_prefix(buf: &[u8]) -> bool {
    let max_len = buf.len().min(1023);
    let slice = &buf[..max_len];
    let slice = match slice.iter().position(|&b| b == 0) {
        Some(nul) => &slice[..nul],
        None => slice,
    };
    slice.starts_with(b"<?xml") && slice.windows(4).any(|w| w == b"<nzb")
}

/// `Scanner::AddExternalFile` filename normalizer (leading `.` -> `_`, UTF-8-aware 200-byte truncation, `.nzb` suffix).
pub fn normalize_external_nzb_name(nzb_name: &[u8], valid_name: &[u8], is_nzb: bool) -> Vec<u8> {
    let mut name = Vec::with_capacity(valid_name.len() + 6);
    if valid_name.first() == Some(&b'.') {
        name.push(b'_');
    }
    name.extend_from_slice(valid_name);

    if name.len() > 200 {
        let ext = match name.iter().rposition(|&b| b == b'.') {
            Some(dot) if name.len() - dot <= 16 => name[dot..].to_vec(),
            _ => Vec::new(),
        };
        let mut cut = 200usize.saturating_sub(ext.len());
        while cut > 0 && (name[cut] & 0xC0) == 0x80 {
            cut -= 1;
        }
        name.truncate(cut);
        name.extend_from_slice(&ext);
    }

    if is_nzb {
        let has_nzb_ext = match nzb_name.iter().rposition(|&b| b == b'.') {
            Some(dot) => eq_ignore_ascii_case(&nzb_name[dot..], b".nzb"),
            None => false,
        };
        if !has_nzb_ext {
            name.extend_from_slice(b".nzb");
        }
    }

    name
}

// ============================================================================
// 4. TwinCheck (`daemon/queue/TwinCheck.cpp`)
// ============================================================================

/// `TwinCheckJob::Sample` layout verifier and sample midpoint index calculator.
/// Returns `Some(Vec<(file_idx, seg_idx)>)` when primary and dupe have identical segment counts and byte sizes.
pub fn twin_sample_indices(
    primary_seg_counts: &[usize],
    primary_seg_bytes: &[i64],
    dupe_seg_counts: &[usize],
    dupe_seg_bytes: &[i64],
    sample_count: usize,
) -> Option<Vec<(usize, usize)>> {
    if primary_seg_counts.is_empty()
        || primary_seg_counts.len() != dupe_seg_counts.len()
        || sample_count == 0
    {
        return None;
    }

    let mut places = Vec::new();
    let mut p_off = 0usize;
    let mut d_off = 0usize;

    for i in 0..primary_seg_counts.len() {
        let p_cnt = primary_seg_counts[i];
        let d_cnt = dupe_seg_counts[i];
        if p_cnt != d_cnt
            || p_off + p_cnt > primary_seg_bytes.len()
            || d_off + d_cnt > dupe_seg_bytes.len()
        {
            return None;
        }
        for k in 0..p_cnt {
            if primary_seg_bytes[p_off + k] != dupe_seg_bytes[d_off + k] {
                return None;
            }
            places.push((i, k));
        }
        p_off += p_cnt;
        d_off += d_cnt;
    }

    if places.is_empty() {
        return None;
    }

    let mut sampled = Vec::with_capacity(sample_count);
    for n in 0..sample_count {
        let idx = ((2 * n + 1) * places.len()) / (2 * sample_count);
        sampled.push(places[idx]);
    }
    Some(sampled)
}

/// `TwinCheck::ServiceWork` primary selector across items sharing a dupe key.
/// 1. Queued & not dupe with highest `dupe_score`
/// 2. First (newest) history item that is not dupe
/// 3. Queued item with highest `dupe_score`
pub fn twin_select_primary(queued: &[u8], is_dupe: &[u8], scores: &[i32]) -> i32 {
    let n = queued.len().min(is_dupe.len()).min(scores.len());
    let mut primary: i32 = -1;

    for i in 0..n {
        if queued[i] != 0
            && is_dupe[i] == 0
            && (primary < 0 || scores[i] > scores[primary as usize])
        {
            primary = i as i32;
        }
    }
    if primary < 0 {
        for i in 0..n {
            if queued[i] == 0 && is_dupe[i] == 0 {
                primary = i as i32;
                break;
            }
        }
    }
    if primary < 0 {
        for i in 0..n {
            if queued[i] != 0 && (primary < 0 || scores[i] > scores[primary as usize]) {
                primary = i as i32;
            }
        }
    }

    primary
}

/// `TwinCheck::ServiceWork` kind label resolver (`"twin"`, `"twin:..."`, `"alt"`, or `""`).
#[allow(clippy::too_many_arguments)]
pub fn twin_resolve_kind(
    is_primary: bool,
    is_dupe: bool,
    by_par2: bool,
    by_files: &[u8],
    listed: bool,
    same_print: bool,
    sampleable: bool,
    sampled_now: bool,
    sampled_val: &[u8],
) -> Vec<u8> {
    if is_primary || !is_dupe {
        return Vec::new();
    }
    if by_par2 {
        if !by_files.is_empty() || listed {
            return by_files.to_vec();
        } else if same_print {
            return b"twin".to_vec();
        } else {
            return b"alt".to_vec();
        }
    }
    if sampleable && sampled_now {
        if sampled_val == b"twin" {
            return b"twin".to_vec();
        } else if sampled_val == b"alt" {
            return b"alt".to_vec();
        }
    }
    Vec::new()
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diskstate_format_and_ids() {
        assert_eq!(
            parse_format_version(b"nzbget diskstate file version 64"),
            64
        );
        assert_eq!(parse_format_version(b"invalid header"), 0);

        let mut line = b"hello\r\nworld\n".to_vec();
        sanitize_line_breaks_in_place(&mut line);
        assert_eq!(&line, b"hello  world ");

        assert_eq!(parse_ids_file(b"nzb 42 file 1080\n"), Some((42, 1080)));
        assert_eq!(parse_ids_file(b"nzb -1 file 5"), None);

        assert_eq!(classify_queue_dir_id(b"n15.log"), Some((15, true)));
        assert_eq!(classify_queue_dir_id(b"204"), Some((204, false)));
        assert_eq!(classify_queue_dir_id(b"204s"), Some((204, false)));
        assert_eq!(classify_queue_dir_id(b"204c"), Some((204, false)));
        assert_eq!(classify_queue_dir_id(b"204x"), None);
    }

    #[test]
    fn test_diskstate_legacy_and_holes() {
        assert_eq!(
            classify_legacy_file_state(
                b"123c",
                b"nzbget diskstate file version 8\n",
                7,
                9
            ),
            Some((123, 2))
        );
        assert_eq!(
            classify_legacy_file_state(
                b"123c",
                b"nzbget diskstate file version 7\n",
                7,
                9
            ),
            None
        );

        assert_eq!(validate_repair_hole(0, 100, 0, 50, 200), Some((100, 50)));
        assert_eq!(validate_repair_hole(0, 160, 0, 50, 200), None);

        assert_eq!(
            parse_unpack_state_lines(
                b"DirectUnpackStatus=1",
                b"UnpackTried=3",
                b"LastUnpackStatus=2",
                1,
                0
            ),
            Some((0, true, true, 2))
        );
        assert_eq!(normalize_post_stage(6, 58, 12), 7);
        assert_eq!(normalize_post_stage(7, 58, 12), 9);
        assert_eq!(normalize_post_stage(14, 64, 12), 12);
    }

    #[test]
    fn test_diskstate_article_and_cleanup() {
        let eval = eval_article_load_state(
            7, false, 5, 0, 1024, 512, 1, 1, true, true, false, false, 0, 10,
        );
        assert_eq!(eval.valid, 1);
        assert_eq!(eval.status, 2);
        assert_eq!(eval.dupe_fallback_round, 1);
        assert_eq!(eval.need_result_filename, 1);
        assert_eq!(eval.completed_articles, 1);

        assert_eq!(classify_temp_file(b"foo.tmp"), (1, 0, 0));
        assert_eq!(classify_temp_file(b"12.003"), (2, 12, 3));
        assert_eq!(classify_temp_file(b"readme.txt"), (0, 0, 0));

        let file_ids = [10, 20, 30];
        let nzb_ids = [1, 2];
        assert!(!is_orphan_queue_file(b"20s", &file_ids, &nzb_ids));
        assert!(is_orphan_queue_file(b"25s", &file_ids, &nzb_ids));
        assert!(!is_orphan_queue_file(b"n2.log", &file_ids, &nzb_ids));
        assert!(is_orphan_queue_file(b"n3.log", &file_ids, &nzb_ids));
    }

    #[test]
    fn test_diskstate_match_servers_and_logs() {
        let servers = [
            ServerMatchInput {
                name: b"Main",
                host: b"news.example.com",
                port: 563,
                user: b"alice",
            },
            ServerMatchInput {
                name: b"RenamedBackup",
                host: b"backup.example.com",
                port: 563,
                user: b"bob",
            },
        ];
        let mut refs = [
            ServerRefEntry {
                state_id: 1,
                name: b"Main",
                host: b"news.example.com",
                port: 563,
                user: b"alice",
                matched: false,
                perfect: false,
            },
            ServerRefEntry {
                state_id: 2,
                name: b"OldBackup",
                host: b"backup.example.com",
                port: 563,
                user: b"bob",
                matched: false,
                perfect: false,
            },
        ];
        let mut out_ids = [0, 0];
        let perfect = match_servers(&servers, &mut refs, &mut out_ids);
        assert!(!perfect);
        assert_eq!(out_ids, [1, 2]);

        let parsed = parse_log_line(b"Tue Nov 3 2026\t1793730000\tWARNING\tDisk almost full");
        assert_eq!(parsed, Some((1793730000, 1, 34)));
    }

    #[test]
    fn test_prepostprocessor_decisions() {
        assert_eq!(can_run_more_jobs(&[], 1, 4, 3, 0), (true, true));
        assert_eq!(can_run_more_jobs(&[3], 1, 4, 3, 1), (true, false));
        assert_eq!(can_run_more_jobs(&[7], 1, 4, 3, 2), (true, true));

        let dec = eval_post_stages(
            true, 0, 0, 2, 0, true, 900, 0, true, 0, true, 0, true, true, true, 0,
        );
        assert_eq!(dec.unpack, 0);
        assert_eq!(dec.skip_unpack_due_to_par, 1);

        assert!(calc_post_pause_state(&[3], true, false, false));
        assert!(!calc_post_pause_state(&[3], false, true, true));
        assert!(calc_post_pause_state(&[5], true, false, false));
        assert!(!calc_post_pause_state(&[5], false, true, true));
        assert!(calc_post_pause_state(&[6], false, true, false));
        assert!(!calc_post_pause_state(&[6], true, false, true));
    }

    #[test]
    fn test_scanner_and_twincheck() {
        assert!(!can_process_extension(b"movie.nzb.queued"));
        assert!(can_process_extension(b"movie.nzb"));
        assert_eq!(classify_missed_articles(2, 50, false), 1);
        assert_eq!(classify_missed_articles(10, 50, false), 2);
        assert!(is_nzb_xml_prefix(b"<?xml version=\"1.0\"?><nzb></nzb>"));

        let norm = normalize_external_nzb_name(b".release", b".release", true);
        assert_eq!(norm, b"_.release.nzb");

        let indices =
            twin_sample_indices(&[2, 2], &[100, 200, 300, 400], &[2, 2], &[100, 200, 300, 400], 2)
                .unwrap();
        assert_eq!(indices, vec![(0, 1), (1, 1)]);

        assert_eq!(twin_select_primary(&[1, 1], &[1, 0], &[500, 100]), 1);
        assert_eq!(
            twin_resolve_kind(false, true, true, b"", false, true, false, false, b""),
            b"twin"
        );
    }
}
