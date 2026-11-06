//! End-to-end Rust implementations of:
//! - `ParDamage` (`daemon/queue/ParDamage.cpp`): per-download PAR2 block damage
//!   tracking, recovery block counting, and early failure projection.
//! - `DupeMatcher` (`daemon/postprocess/DupeMatcher.cpp`): directory scanning for
//!   the largest file, RAR archive inspection handoff, and size comparison.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use crate::dupefallback::{blocks_touched, list_par2_files, project_par_damage, NzbgetRsByteRange};

pub const MAX_PAR2_BLOCK_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct FileDamage {
    pub name: Vec<u8>,
    pub size: i64,
    pub holes: Vec<NzbgetRsByteRange>,
}

#[derive(Debug, Default)]
pub struct ParDamageState {
    pub files: Vec<FileDamage>,
    pub done_bytes: i64,
    pub par_files_done: i32,
    pub par_files_read: i32,
    pub par2: Vec<(Vec<u8>, u64, u64)>,
    pub reported: bool,
}

impl ParDamageState {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            done_bytes: 0,
            par_files_done: 0,
            par_files_read: -1,
            par2: Vec::new(),
            reported: false,
        }
    }
}

static STATES: LazyLock<Mutex<HashMap<i32, ParDamageState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn base_file_name(path: &[u8]) -> &[u8] {
    path.iter()
        .rposition(|&b| b == b'/' || b == b'\\')
        .map_or(path, |pos| &path[pos + 1..])
}

/// Estimates the decoded hole size (`file_size * 100 / 102`) for a file with zero
/// arrived articles in `ParDamage::FileCompleted`.
#[inline]
pub fn estimate_unarrived_decoded_size(file_size: i64) -> i64 {
    file_size.wrapping_mul(100) / 102
}

/// Evaluates whether a `Par2Desc` entry matches a `FileDamage` entry in `ParDamage::Judge`.
#[inline]
pub fn desc_matches_file_damage(
    block_size: u64,
    max_par2_block_size: u64,
    desc_name: &[u8],
    damage_name: &[u8],
    damage_size: i64,
    desc_length: u64,
) -> bool {
    block_size > 0
        && block_size % 4 == 0
        && block_size <= max_par2_block_size
        && (desc_name == damage_name || (damage_size > 0 && (desc_length as i64) == damage_size))
}

/// Computes the recovery blocks contributed by a PAR2 volume file in the queue.
#[inline]
pub fn calc_queued_volume_blocks(parsed_blocks: i32, file_size: i64, block_size: u64) -> i32 {
    if parsed_blocks >= 0 {
        parsed_blocks
    } else {
        let denom = block_size.saturating_add(68) as i64;
        if denom <= 0 {
            0
        } else {
            ((file_size.saturating_mul(100) / 102) / denom) as i32
        }
    }
}

/// Computes the recovery blocks contributed by a completed PAR2 volume file on disk.
#[inline]
pub fn calc_completed_volume_blocks(parsed_blocks: i32, disk_size: i64, block_size: u64) -> i32 {
    if parsed_blocks >= 0 {
        parsed_blocks
    } else if disk_size > 0 {
        let denom = block_size.saturating_add(68) as i64;
        if denom <= 0 {
            0
        } else {
            (disk_size / denom) as i32
        }
    } else {
        0
    }
}

/// Records completion of a file in `ParDamage::FileCompleted`.
#[allow(clippy::too_many_arguments)]
pub fn file_completed(
    nzb_id: i32,
    is_par: bool,
    failed_plus_missed: i32,
    decoded_size: i64,
    success_articles: i32,
    file_size: i64,
    filename: &[u8],
    holes: &[NzbgetRsByteRange],
) {
    let mut guard = STATES.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.entry(nzb_id).or_insert_with(ParDamageState::new);

    if is_par {
        state.par_files_done += 1;
        return;
    }

    state.done_bytes = state.done_bytes.wrapping_add(file_size);
    if failed_plus_missed == 0 {
        return;
    }

    let name = base_file_name(filename).to_vec();
    let damage = if decoded_size > 0 {
        FileDamage {
            name,
            size: decoded_size,
            holes: holes.to_vec(),
        }
    } else if success_articles == 0 {
        FileDamage {
            name,
            size: 0,
            holes: vec![NzbgetRsByteRange {
                offset: 0,
                size: estimate_unarrived_decoded_size(file_size),
            }],
        }
    } else {
        return;
    };

    if !damage.holes.is_empty() {
        state.files.push(damage);
    }
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ParDamageVerdictC {
    pub known: i32,
    pub block_size: u64,
    pub damaged_blocks: i32,
    pub recovery_blocks: i32,
    pub done_bytes: i64,
    pub total_bytes: i64,
    pub projected_blocks: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct QueuedParCandidate<'a> {
    pub filename: &'a [u8],
    pub confirmed: bool,
    pub size: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct CompletedParCandidate<'a> {
    pub filename: &'a [u8],
    pub not_failed: bool,
}

#[cfg(unix)]
fn bytes_to_os_str(bytes: &[u8]) -> &std::ffi::OsStr {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes)
}

#[cfg(not(unix))]
fn bytes_to_path_buf(bytes: &[u8]) -> std::path::PathBuf {
    std::path::PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// Fast check used by `ParDamage::Judge` to avoid snapshotting queued/completed
/// file lists when no damaged files have been recorded for `nzb_id`.
pub fn has_damage(nzb_id: i32) -> bool {
    let guard = STATES.lock().unwrap_or_else(|e| e.into_inner());
    guard.get(&nzb_id).is_some_and(|s| !s.files.is_empty())
}

/// Evaluates `ParDamage::Judge` for download `nzb_id`.
pub fn judge<F>(
    nzb_id: i32,
    dest_dir: &Path,
    nzb_size: i64,
    nzb_par_size: i64,
    queued: &[QueuedParCandidate<'_>],
    completed: &[CompletedParCandidate<'_>],
    lower: &F,
) -> ParDamageVerdictC
where
    F: Fn(u8) -> u8 + ?Sized,
{
    let mut verdict = ParDamageVerdictC::default();
    let mut guard = STATES.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = guard.get_mut(&nzb_id) else {
        return verdict;
    };
    if state.files.is_empty() {
        return verdict;
    }

    if state.par_files_read != state.par_files_done {
        state.par2 = list_par2_files(dest_dir);
        state.par_files_read = state.par_files_done;
    }

    for damage in &state.files {
        let mut matched: Option<&(Vec<u8>, u64, u64)> = None;
        for desc in &state.par2 {
            if desc_matches_file_damage(
                desc.2,
                MAX_PAR2_BLOCK_SIZE,
                &desc.0,
                &damage.name,
                damage.size,
                desc.1,
            ) {
                matched = Some(desc);
                break;
            }
        }
        let Some(m) = matched else {
            continue;
        };
        if verdict.block_size == 0 {
            verdict.block_size = m.2;
        }
        if damage.size > 0 {
            verdict.damaged_blocks += blocks_touched(&damage.holes, m.2);
        } else {
            verdict.damaged_blocks += blocks_touched(
                &[NzbgetRsByteRange {
                    offset: 0,
                    size: m.1 as i64,
                }],
                m.2,
            );
        }
    }

    if verdict.block_size == 0 {
        return verdict;
    }
    verdict.known = 1;

    let block_size = verdict.block_size;
    for q in queued {
        if let Some((_, blocks)) = crate::parparser::parse_par_filename(q.filename, q.confirmed, lower) {
            verdict.recovery_blocks += calc_queued_volume_blocks(blocks, q.size, block_size);
        }
    }
    for c in completed {
        if !c.not_failed {
            continue;
        }
        if let Some((_, mut blocks)) = crate::parparser::parse_par_filename(c.filename, true, lower) {
            if blocks < 0 {
                #[cfg(unix)]
                let full_path = dest_dir.join(bytes_to_os_str(c.filename));
                #[cfg(not(unix))]
                let full_path = dest_dir.join(bytes_to_path_buf(c.filename));
                let disk_size = std::fs::metadata(&full_path)
                    .ok()
                    .filter(|m| !m.is_dir())
                    .map(|m| m.len() as i64)
                    .unwrap_or(-1);
                blocks = calc_completed_volume_blocks(blocks, disk_size, block_size);
            }
            verdict.recovery_blocks += blocks;
        }
    }

    verdict.done_bytes = state.done_bytes;
    verdict.total_bytes = nzb_size.wrapping_sub(nzb_par_size).max(0);
    let (proj, _, _) = project_par_damage(
        true,
        verdict.damaged_blocks,
        verdict.recovery_blocks,
        verdict.done_bytes,
        verdict.total_bytes,
    );
    verdict.projected_blocks = proj;
    verdict
}

/// `ParDamage::FirstReport`: returns `true` the first time called for `nzb_id`, `false` thereafter.
pub fn first_report(nzb_id: i32) -> bool {
    let mut guard = STATES.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.entry(nzb_id).or_insert_with(ParDamageState::new);
    let first = !state.reported;
    state.reported = true;
    first
}

/// `ParDamage::Forget`: removes any stored state for `nzb_id`.
pub fn forget(nzb_id: i32) {
    let mut guard = STATES.lock().unwrap_or_else(|e| e.into_inner());
    guard.remove(&nzb_id);
}

// ---- DupeMatcher (`daemon/postprocess/DupeMatcher.cpp`) ----

#[cfg(unix)]
fn os_str_bytes(s: &std::ffi::OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes()
}

#[cfg(not(unix))]
fn os_str_bytes(s: &std::ffi::OsStr) -> &[u8] {
    s.to_str().map(str::as_bytes).unwrap_or(b"")
}

/// Scans `directory` for the largest file (`DupeMatcher::FindLargestFile`).
/// If any `.rar` file is encountered, delegates to `on_rar_archive(rar_expected_size)`
/// and stops scanning immediately. When `expected_rar_size` is `None` (`Prepare()`),
/// `rar_expected_size` tracks the live `*max_size` accumulated so far; when `Some(sz)`
/// (`MatchDupeContent()`), `rar_expected_size` uses the prepared `m_maxSize`.
pub fn find_largest_file<R>(
    directory: &Path,
    filename_buf: &mut [u8],
    max_size: &mut i64,
    compressed: &mut bool,
    expected_rar_size: Option<i64>,
    mut on_rar_archive: R,
) where
    R: FnMut(&mut [u8], i64, &mut i64, &mut bool),
{
    *max_size = 0;
    *compressed = false;

    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        let file_size = std::fs::metadata(&entry_path)
            .ok()
            .filter(|m| !m.is_dir())
            .map(|m| m.len() as i64)
            .unwrap_or(-1);

        let file_name = entry.file_name();
        let name_bytes = os_str_bytes(&file_name);
        let ext = crate::filetypes::file_extension(name_bytes);

        if ext.eq_ignore_ascii_case(b".zip") {
            if let Ok(info) = crate::unpack::inspect_zip_file(&entry_path) {
                if let Some((inner_name, inner_size, is_compressed)) = info.largest_file {
                    let inner_size_i64 = inner_size as i64;
                    if inner_size_i64 > *max_size {
                        *max_size = inner_size_i64;
                        *compressed = is_compressed;
                        if !filename_buf.is_empty() {
                            let inner_bytes = inner_name.as_bytes();
                            let copy_len = inner_bytes.len().min(filename_buf.len() - 1);
                            filename_buf[..copy_len].copy_from_slice(&inner_bytes[..copy_len]);
                            for b in &mut filename_buf[copy_len..] {
                                *b = 0;
                            }
                        }
                    }
                    continue;
                }
            }
        }

        if file_size > *max_size {
            *max_size = file_size;
            *compressed = false;
            if !filename_buf.is_empty() {
                let copy_len = name_bytes.len().min(filename_buf.len() - 1);
                filename_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
                for b in &mut filename_buf[copy_len..] {
                    *b = 0;
                }
            }
        }

        if crate::filetypes::is_rar_ext(ext) {
            let rar_expected = expected_rar_size.unwrap_or(*max_size);
            on_rar_archive(filename_buf, rar_expected, max_size, compressed);
            return;
        }
    }
}

/// Returns the suffix logged by `DupeMatcher::Prepare` and `DupeMatcher::MatchDupeContent`.
pub fn size_mismatch_suffix(ok: bool) -> &'static std::ffi::CStr {
    if ok {
        c""
    } else {
        c", size mismatch"
    }
}

/// Evaluates `DupeMatcher::MatchDupeContent`'s size and compression match condition.
pub fn is_dupe_content_match(
    dupe_max_size: i64,
    max_size: i64,
    dupe_compressed: bool,
    compressed: bool,
) -> bool {
    dupe_max_size == max_size && dupe_compressed == compressed
}

fn format_dupe_matcher_detail_msg(filename_buf: &[u8], max_size: i64, ok: bool) -> Vec<u8> {
    let end = filename_buf
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(filename_buf.len());
    let fname = &filename_buf[..end];
    let suffix = size_mismatch_suffix(ok).to_bytes();
    let mut msg = Vec::with_capacity(36 + fname.len() + suffix.len());
    msg.extend_from_slice(b"Found main file ");
    msg.extend_from_slice(fname);
    msg.extend_from_slice(format!(" with size {max_size} bytes").as_bytes());
    msg.extend_from_slice(suffix);
    msg
}

#[cfg(unix)]
extern "C" {
    fn kill(pid: std::ffi::c_int, sig: std::ffi::c_int) -> std::ffi::c_int;
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as std::ffi::c_int;
        if pid > 0 {
            unsafe {
                kill(-pid, 9);
            }
        }
    }
    let _ = child.kill();
}

/// Runs `RarLister::FindLargestFile` (`<unrar> lt *.rar` in `directory` with a bounded timeout),
/// updating `filename_buf`, `max_size`, and `compressed`, and emitting `"Reading file ..."`
/// detail log messages via `on_detail_log`.
pub fn run_rar_lister_in_dir<L>(
    directory: &Path,
    unrar_cmd: &[u8],
    filename_buf: &mut [u8],
    rar_expected_size: i64,
    timeout_sec: u64,
    max_size: &mut i64,
    compressed: &mut bool,
    mut on_detail_log: L,
) -> bool
where
    L: FnMut(&[u8]),
{
    let cmd_args = crate::util::split_command_line(unrar_cmd);
    let Some(first_arg) = cmd_args.first().filter(|a| !a.is_empty()) else {
        return false;
    };
    #[cfg(unix)]
    let unrar_prog = {
        use std::os::unix::ffi::OsStrExt;
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(first_arg))
    };
    #[cfg(not(unix))]
    let unrar_prog = std::path::PathBuf::from(String::from_utf8_lossy(first_arg).into_owned());

    *max_size = -1;
    *compressed = false;

    let mut cmd = std::process::Command::new(&unrar_prog);
    cmd.current_dir(directory)
        .args(["lt", "*.rar"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }

    let Ok(mut child) = cmd.spawn() else {
        return true;
    };

    let Some(stdout) = child.stdout.take() else {
        kill_process_tree(&mut child);
        let _ = child.wait();
        return true;
    };

    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let reader_handle = std::thread::spawn(move || {
        use std::io::BufRead as _;
        let mut reader = std::io::BufReader::new(stdout);
        let mut buf = Vec::new();
        while let Ok(n) = reader.read_until(b'\n', &mut buf) {
            if n == 0 {
                break;
            }
            while matches!(buf.last(), Some(b'\n' | b'\r')) {
                buf.pop();
            }
            if tx.send(std::mem::take(&mut buf)).is_err() {
                break;
            }
        }
        drop(done_tx);
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_sec);
    let mut last_size_max: std::ffi::c_int = 0;
    let mut comp_flag: std::ffi::c_int = 0;
    let mut last_filename = Vec::<u8>::new();
    let mut killed = false;

    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            kill_process_tree(&mut child);
            killed = true;
            break;
        }
        let wait_dur = (deadline - now).min(std::time::Duration::from_millis(200));
        match rx.recv_timeout(wait_dur) {
            Ok(line) => {
                let mut c_line = line.clone();
                if let Some(nul) = c_line.iter().position(|&b| b == 0) {
                    c_line.truncate(nul);
                }
                c_line.push(0);
                let action = unsafe {
                    crate::postrepair::parse_rar_lister_line_cstr(
                        c_line.as_ptr().cast(),
                        rar_expected_size,
                        max_size,
                        &mut comp_flag,
                        &mut last_size_max,
                    )
                };
                *compressed = comp_flag != 0;
                if action == 1 && line.len() >= 9 {
                    let mut msg = Vec::with_capacity(13 + line.len() - 9);
                    msg.extend_from_slice(b"Reading file ");
                    msg.extend_from_slice(&line[9..]);
                    on_detail_log(&msg);
                } else if action == 2 && line.len() >= 14 {
                    last_filename = line[14..].to_vec();
                } else if action == 3 {
                    let n = last_filename.len().min(filename_buf.len().saturating_sub(1));
                    filename_buf[..n].copy_from_slice(&last_filename[..n]);
                    if !filename_buf.is_empty() {
                        filename_buf[n] = 0;
                    }
                } else if action == 5 {
                    kill_process_tree(&mut child);
                    killed = true;
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    drop(rx);
    let mut reaped = false;
    if !killed {
        loop {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => {
                    reaped = true;
                    break;
                }
                Ok(None) => {
                    let now = std::time::Instant::now();
                    if now >= deadline {
                        kill_process_tree(&mut child);
                        break;
                    }
                    std::thread::sleep(
                        (deadline - now).min(std::time::Duration::from_millis(10)),
                    );
                }
            }
        }
    }
    if !reaped {
        let _ = child.wait();
    }

    let reader_wait = deadline
        .saturating_duration_since(std::time::Instant::now())
        .max(std::time::Duration::from_millis(20));
    if matches!(
        done_rx.recv_timeout(reader_wait),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ) {
        let _ = reader_handle.join();
    }
    true
}

/// `DupeMatcher::Prepare` (`daemon/postprocess/DupeMatcher.cpp`).
pub fn dupe_matcher_prepare<L>(
    dest_dir: &Path,
    unrar_cmd: &[u8],
    expected_size: i64,
    max_size: &mut i64,
    compressed: &mut bool,
    mut on_detail_log: L,
) -> bool
where
    L: FnMut(&[u8]),
{
    let mut filename = [0u8; 1024];
    find_largest_file(
        dest_dir,
        &mut filename,
        max_size,
        compressed,
        None,
        |buf, rar_expected, ms, comp| {
            run_rar_lister_in_dir(
                dest_dir,
                unrar_cmd,
                buf,
                rar_expected,
                60,
                ms,
                comp,
                &mut on_detail_log,
            );
        },
    );
    let size_ok = crate::postrepair::size_diff_ok(*max_size, expected_size, 20);
    let msg = format_dupe_matcher_detail_msg(&filename, *max_size, size_ok);
    on_detail_log(&msg);
    size_ok
}

/// `DupeMatcher::MatchDupeContent` (`daemon/postprocess/DupeMatcher.cpp`).
pub fn dupe_matcher_match_dupe_content<L>(
    dupe_dir: &Path,
    unrar_cmd: &[u8],
    max_size: i64,
    compressed: bool,
    mut on_detail_log: L,
) -> bool
where
    L: FnMut(&[u8]),
{
    let mut filename = [0u8; 1024];
    let mut dupe_max_size = 0i64;
    let mut dupe_compressed = false;
    find_largest_file(
        dupe_dir,
        &mut filename,
        &mut dupe_max_size,
        &mut dupe_compressed,
        Some(max_size),
        |buf, rar_expected, ms, comp| {
            run_rar_lister_in_dir(
                dupe_dir,
                unrar_cmd,
                buf,
                rar_expected,
                60,
                ms,
                comp,
                &mut on_detail_log,
            );
        },
    );
    let ok = is_dupe_content_match(dupe_max_size, max_size, dupe_compressed, compressed);
    let msg = format_dupe_matcher_detail_msg(&filename, max_size, ok);
    on_detail_log(&msg);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_par_damage_end_to_end() {
        let dir = std::env::temp_dir().join(format!("rustyget_pardamage_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let nzb_id = 999_123;
        forget(nzb_id);
        assert!(!has_damage(nzb_id));
        assert!(first_report(nzb_id));
        assert!(!first_report(nzb_id));

        // Record a completed PAR2 file and a damaged data file
        file_completed(nzb_id, true, 0, 1000, 1, 1020, b"movie.par2", &[]);
        assert!(!has_damage(nzb_id));
        file_completed(
            nzb_id,
            false,
            2,
            10_000,
            8,
            10_200,
            b"/tmp/movie.mkv",
            &[NzbgetRsByteRange {
                offset: 0,
                size: 4000,
            }],
        );
        assert!(has_damage(nzb_id));

        // Without par2 packets on disk, Judge returns unknown
        let v = judge(
            nzb_id,
            &dir,
            20_000,
            2_000,
            &[],
            &[],
            &|b: u8| b.to_ascii_lowercase(),
        );
        assert_eq!(v.known, 0);

        forget(nzb_id);
        assert!(first_report(nzb_id));
        forget(nzb_id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_find_largest_file_and_dupe_match() {
        let dir = std::env::temp_dir().join(format!("rustyget_dupematcher_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("small.nfo"), vec![b'a'; 100]).unwrap();
        std::fs::write(dir.join("main.mkv"), vec![b'b'; 5000]).unwrap();

        let mut buf = [0u8; 128];
        let mut max_size = 0;
        let mut compressed = false;
        find_largest_file(
            &dir,
            &mut buf,
            &mut max_size,
            &mut compressed,
            None,
            |_, _, _, _| unreachable!("no rar files"),
        );
        assert_eq!(max_size, 5000);
        assert!(!compressed);
        let name_end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        assert_eq!(&buf[..name_end], b"main.mkv");
        assert!(is_dupe_content_match(5000, 5000, false, false));
        assert!(!is_dupe_content_match(5000, 4500, false, false));
        assert!(!is_dupe_content_match(5000, 5000, true, false));

        // Verify live vs prepared expected_rar_size when a .rar file is present
        std::fs::write(dir.join("archive.rar"), vec![b'r'; 200]).unwrap();
        let mut seen_live = -1;
        find_largest_file(
            &dir,
            &mut buf,
            &mut max_size,
            &mut compressed,
            None,
            |_, exp_sz, _, _| {
                seen_live = exp_sz;
            },
        );
        assert!(seen_live >= 200);

        let mut seen_prepared = -1;
        find_largest_file(
            &dir,
            &mut buf,
            &mut max_size,
            &mut compressed,
            Some(99_999),
            |_, exp_sz, _, _| {
                seen_prepared = exp_sz;
            },
        );
        assert_eq!(seen_prepared, 99_999);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dupe_matcher_full_suite_port_from_cpp() {
        let env_path = std::env::var("PATH").unwrap_or_default();
        let Some(unrar_bytes) =
            crate::util::util_resolve_path_from_env(b"unrar", env_path.as_bytes())
        else {
            return;
        };
        let testdata_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/testdata");
        let work_dir = std::env::temp_dir().join(format!(
            "nzbget_rust_dupematcher_suite_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&work_dir);
        std::fs::create_dir_all(&work_dir).unwrap();

        let copy_dir = |src: &Path, dst: &Path| {
            std::fs::create_dir_all(dst).unwrap();
            for entry in std::fs::read_dir(src).unwrap().flatten() {
                if entry.path().is_file() {
                    std::fs::copy(entry.path(), dst.join(entry.file_name())).unwrap();
                }
            }
        };

        let dupe1 = work_dir.join("dupe1");
        copy_dir(&testdata_dir.join("parchecker"), &dupe1);
        let dupe2 = work_dir.join("dupe2");
        copy_dir(&testdata_dir.join("parchecker"), &dupe2);
        let _ = std::fs::remove_file(dupe2.join("testfile.nfo"));
        let rardupe1 = testdata_dir.join("dupematcher1");
        let rardupe2 = testdata_dir.join("dupematcher2");
        let nondupe = work_dir.join("nondupe");
        copy_dir(&testdata_dir.join("parchecker"), &nondupe);
        let _ = std::fs::remove_file(nondupe.join("testfile.dat"));

        let expected_size = std::fs::metadata(dupe1.join("testfile.dat"))
            .unwrap()
            .len() as i64;

        struct RustDupeMatcher<'a> {
            dest_dir: std::path::PathBuf,
            unrar_cmd: &'a [u8],
            expected_size: i64,
            max_size: i64,
            compressed: bool,
        }

        impl<'a> RustDupeMatcher<'a> {
            fn new(dest_dir: &Path, expected_size: i64, unrar_cmd: &'a [u8]) -> Self {
                Self {
                    dest_dir: dest_dir.to_path_buf(),
                    unrar_cmd,
                    expected_size,
                    max_size: -1,
                    compressed: false,
                }
            }

            fn prepare(&mut self) -> bool {
                let dir = self.dest_dir.clone();
                dupe_matcher_prepare(
                    &dir,
                    self.unrar_cmd,
                    self.expected_size,
                    &mut self.max_size,
                    &mut self.compressed,
                    |_| {},
                )
            }

            fn match_dupe_content(&self, dupe_dir: &Path) -> bool {
                dupe_matcher_match_dupe_content(
                    dupe_dir,
                    self.unrar_cmd,
                    self.max_size,
                    self.compressed,
                    |_| {},
                )
            }
        }

        let mut dupe1_m = RustDupeMatcher::new(&dupe1, expected_size, &unrar_bytes);
        assert!(dupe1_m.prepare());
        assert!(dupe1_m.match_dupe_content(&dupe2));
        assert!(dupe1_m.match_dupe_content(&rardupe1));
        assert!(dupe1_m.match_dupe_content(&rardupe2));
        assert!(!dupe1_m.match_dupe_content(&nondupe));

        let mut dupe2_m = RustDupeMatcher::new(&dupe2, expected_size, &unrar_bytes);
        assert!(dupe2_m.prepare());
        assert!(dupe2_m.match_dupe_content(&dupe1));
        assert!(dupe2_m.match_dupe_content(&rardupe1));
        assert!(dupe2_m.match_dupe_content(&rardupe2));
        assert!(!dupe2_m.match_dupe_content(&nondupe));

        let mut non_dupe_m = RustDupeMatcher::new(&nondupe, expected_size, &unrar_bytes);
        assert!(!non_dupe_m.prepare());
        assert!(!non_dupe_m.match_dupe_content(&dupe1));
        assert!(!non_dupe_m.match_dupe_content(&dupe2));
        assert!(!non_dupe_m.match_dupe_content(&rardupe1));
        assert!(!non_dupe_m.match_dupe_content(&rardupe2));

        let mut rardupe1_m = RustDupeMatcher::new(&rardupe1, expected_size, &unrar_bytes);
        assert!(rardupe1_m.prepare());
        assert!(rardupe1_m.match_dupe_content(&dupe1));
        assert!(rardupe1_m.match_dupe_content(&dupe2));
        assert!(rardupe1_m.match_dupe_content(&rardupe2));
        assert!(!rardupe1_m.match_dupe_content(&nondupe));

        let mut rardupe2_m = RustDupeMatcher::new(&rardupe2, expected_size, &unrar_bytes);
        assert!(rardupe2_m.prepare());
        assert!(rardupe2_m.match_dupe_content(&rardupe1));
        assert!(rardupe2_m.match_dupe_content(&dupe1));
        assert!(rardupe2_m.match_dupe_content(&dupe2));
        assert!(!rardupe2_m.match_dupe_content(&nondupe));

        let _ = std::fs::remove_dir_all(&work_dir);
    }

    #[test]
    fn run_rar_lister_in_dir_enforces_timeout_on_wrapper_descendants_and_closed_stdout() {
        use std::os::unix::fs::PermissionsExt as _;
        let work_dir = std::env::temp_dir().join(format!(
            "nzbget_rs_rarlister_timeout_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&work_dir);
        std::fs::create_dir_all(&work_dir).unwrap();

        // Case 1: wrapper leaves a background descendant holding stdout open
        let wrapper1 = work_dir.join("wrap_descendant.sh");
        std::fs::write(&wrapper1, b"#!/bin/sh\nsleep 30 &\nexit 0\n").unwrap();
        std::fs::set_permissions(&wrapper1, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut filename = [0u8; 64];
        let mut max_size = -1;
        let mut compressed = false;
        let start1 = std::time::Instant::now();
        assert!(run_rar_lister_in_dir(
            &work_dir,
            wrapper1.to_string_lossy().as_bytes(),
            &mut filename,
            100,
            1,
            &mut max_size,
            &mut compressed,
            |_| {},
        ));
        assert!(start1.elapsed() < std::time::Duration::from_secs(5));

        // Case 2: wrapper closes stdout immediately then sleeps
        let wrapper2 = work_dir.join("wrap_closed_stdout.sh");
        std::fs::write(&wrapper2, b"#!/bin/sh\nexec >/dev/null 2>&1\nsleep 30\n").unwrap();
        std::fs::set_permissions(&wrapper2, std::fs::Permissions::from_mode(0o755)).unwrap();
        let start2 = std::time::Instant::now();
        assert!(run_rar_lister_in_dir(
            &work_dir,
            wrapper2.to_string_lossy().as_bytes(),
            &mut filename,
            100,
            1,
            &mut max_size,
            &mut compressed,
            |_| {},
        ));
        assert!(start2.elapsed() < std::time::Duration::from_secs(5));

        let _ = std::fs::remove_dir_all(&work_dir);
    }
}
