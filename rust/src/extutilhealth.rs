//! Extension, Utility, NServ & SystemHealth Hot-Path Port (`rust/src/extutilhealth.rs`).
//!
//! Covers hot-path computational, parsing, classification, and validation kernels for:
//! - `daemon/util/DataAnalytics.cpp`: in-place sort, min/max/avg/median/percentiles
//!   (`Compute`, `CalculatePercentile`)
//! - `daemon/util/Benchmark.cpp`: free-space reservation clamp and write-chunk size calculation
//!   (`LimitByFreeSpace`, `Run`)
//! - `daemon/util/Unrar.cpp`: overwrite switch selection, `.part0*1.rar` filter, and exit code
//!   error message decoding (`MakeArgs`, `IsSupported`, `DecodeExitCode`)
//! - `daemon/util/SevenZip.cpp`: overwrite switch selection and exit code error message decoding
//!   (`MakeArgs`, `DecodeExitCode`)
//! - `daemon/util/Service.cpp`: service interval readiness and remaining wait calculation
//!   (`ServiceCoordinator::Run`)
//! - `daemon/main/DiskService.cpp`: service interval calculation and low-space threshold check
//!   (`ServiceInterval`, `CheckDiskSpace`)
//! - `daemon/nserv/NServFrontend.cpp`: log buffer start index and skip detection (`Update`)
//! - `daemon/extension/CommandScript.cpp`: exit code classification (`ExecuteScript`)
//! - `daemon/extension/SchedulerScript.cpp`: external process log prefix extension stripping
//!   (`ExecuteExternalProcess`)
//! - `daemon/extension/NzbScript.cpp`: environment parameter prefix matching and stripping
//!   (`PrepareEnvParameters`)
//! - `daemon/extension/FeedScript.cpp`: feed script exit code check (`ExecuteScript`)
//! - `daemon/systemhealth/IncomingNzbValidator.cpp`: `NzbDirInterval`, `NzbDirFileAge`,
//!   `DupeCheck`, and `DupeSearch` validators
//! - `daemon/systemhealth/UnpackValidator.cpp`: `DirectUnpack`, `ExtensionCleanup`, and
//!   `UnpackIgnoreExt` validators
//! - `daemon/systemhealth/ExtensionScriptsValidator.cpp`: script extension stripping,
//!   `ShellOverride` `=` splitting, and `EventInterval` validation
//! - `daemon/systemhealth/NewsServersValidator.cpp`: primary server active check
//!   (`AnyPrimaryServerExistsValidator`)
//! - `daemon/systemhealth/DisplayValidator.cpp`: `OutputMode` and `UpdateInterval` validators
//! - `daemon/systemhealth/SchedulerTaskValidator.cpp`: `TimeValidator` and `WeekDaysValidator`
//! - `daemon/systemhealth/CategoryValidator.cpp`: `Category::UnpackValidator`
//! - `daemon/systemhealth/Status.cpp`: `SeverityToStr`

use std::ffi::CStr;

// ============================================================================
// DataAnalytics (`daemon/util/DataAnalytics.cpp`)
// ============================================================================

/// Matches `DataAnalytics::CalculatePercentile(percentile)` on an already-sorted slice.
pub fn data_analytics_percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if percentile == 0.0 {
        return sorted[0];
    }
    if percentile == 1.0 {
        return sorted[sorted.len() - 1];
    }

    let rank = percentile * ((sorted.len() - 1) as f64);
    let idx = rank as usize;
    let fraction = rank - (idx as f64);

    if idx + 1 < sorted.len() {
        sorted[idx] * (1.0 - fraction) + sorted[idx + 1] * fraction
    } else {
        sorted[idx]
    }
}

/// Matches `DataAnalytics::Compute()` in-place on `measurements`, returning
/// `(min, max, avg, median, p25, p75, p90)`.
pub fn data_analytics_compute(
    measurements: &mut [f64],
) -> Option<(f64, f64, f64, f64, f64, f64, f64)> {
    if measurements.is_empty() {
        return None;
    }
    measurements.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min = measurements[0];
    let max = measurements[measurements.len() - 1];
    let sum: f64 = measurements.iter().copied().fold(0.0, |acc, x| acc + x);
    let avg = sum / (measurements.len() as f64);
    let median = data_analytics_percentile(measurements, 0.5);
    let p25 = data_analytics_percentile(measurements, 0.25);
    let p75 = data_analytics_percentile(measurements, 0.75);
    let p90 = data_analytics_percentile(measurements, 0.90);
    Some((min, max, avg, median, p25, p75, p90))
}

// ============================================================================
// Benchmark (`daemon/util/Benchmark.cpp`)
// ============================================================================

/// Matches `DiskBenchmark::LimitByFreeSpace(requested, available)`.
pub fn benchmark_limit_by_free_space(requested: u64, available: u64, reserve: u64) -> u64 {
    if available <= reserve {
        0
    } else {
        requested.min(available - reserve)
    }
}

/// Matches the per-iteration write chunk calculation in `DiskBenchmark::Run`:
/// `static_cast<size_t>(std::min<uint64_t>(blockSize, maxBytes - result.writeBytes))`.
pub fn benchmark_calc_write_chunk(block_size: usize, max_bytes: u64, write_bytes: u64) -> usize {
    if write_bytes >= max_bytes {
        return 0;
    }
    let rem = max_bytes - write_bytes;
    (block_size as u64).min(rem) as usize
}

// ============================================================================
// Unrar & SevenZip (`daemon/util/Unrar.cpp`, `SevenZip.cpp`)
// ============================================================================

/// Matches the overwrite mode switch in `Unrar::MakeArgs()`:
/// `0 (Skip) -> "-o-"`, `1 (Overwrite) -> "-o+"`, `2 (AutoRename) -> "-or"`.
pub fn unrar_overwrite_switch(mode: i32) -> &'static CStr {
    match mode {
        0 => c"-o-",
        1 => c"-o+",
        2 => c"-or",
        _ => c"-o-",
    }
}

/// Matches the `.part` / `\\.part0*1\\.rar$` check in `Unrar::IsSupported` after
/// `FileTypes::IsRarExt` has already returned `true`.
pub fn unrar_is_part1_or_unsplit_rar(filename: &[u8]) -> bool {
    // Check if lowercase filename contains ".part"
    let has_part = filename
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b".part"));
    if !has_part {
        return true;
    }

    // Must match `\.part0*1\.rar$` case-insensitively at the end of `filename`.
    if filename.len() < 10 {
        return false;
    }
    let suffix_start = filename.len() - 4;
    if !filename[suffix_start..].eq_ignore_ascii_case(b".rar") {
        return false;
    }
    let before_rar = &filename[..suffix_start];
    if !before_rar.ends_with(b"1") {
        return false;
    }
    let mut i = before_rar.len() - 1;
    while i > 0 && before_rar[i - 1] == b'0' {
        i -= 1;
    }
    if i < 5 {
        return false;
    }
    before_rar[i - 5..i].eq_ignore_ascii_case(b".part")
}

/// Matches `Unrar::DecodeExitCode(ec)`:
/// Returns `(is_success, known_error_msg)`. If `is_success` is `true`, no error is logged.
/// If `is_success` is `false` and `known_error_msg` is empty, caller logs `"Unknown error %d"`.
pub fn unrar_decode_exit_code(ec: i32) -> (bool, &'static CStr) {
    match ec {
        0 => (true, c""),
        1 => (
            false,
            c"Extraction finished, but some files might be missing or incomplete",
        ),
        2 => (
            false,
            c"The process could not start or was interrupted unexpectedly",
        ),
        3 => (
            false,
            c"The archive is damaged. The extracted files are likely corrupt",
        ),
        4 => (
            false,
            c"This archive is locked and cannot be modified or unpacked",
        ),
        5 => (
            false,
            c"Could not write files to the destination. Please check your permissions and disk space",
        ),
        6 => (
            false,
            c"Couldn't open the archive. The file may be missing or you don't have permission to read it",
        ),
        7 => (false, c"An internal program error occurred"),
        8 => (
            false,
            c"Your computer ran out of memory. Please try closing other applications first",
        ),
        9 => (
            false,
            c"Couldn't create files in the destination folder. Please check your permissions",
        ),
        10 => (
            false,
            c"There were no files inside the archive to extract",
        ),
        11 => (false, c"The password you entered was incorrect"),
        _ => (false, c""),
    }
}

/// Matches the overwrite mode switch in `SevenZip::MakeArgs()`:
/// `0 (Skip) -> "-aos"`, `1 (Overwrite) -> "-aoa"`, `2 (AutoRename) -> "-aou"`.
pub fn sevenzip_overwrite_switch(mode: i32) -> &'static CStr {
    match mode {
        0 => c"-aos",
        1 => c"-aoa",
        2 => c"-aou",
        _ => c"-aos",
    }
}

/// Matches `SevenZip::DecodeExitCode(ec)`:
/// Returns `(is_success, known_error_msg)`. If `is_success` is `false` and `known_error_msg` is
/// empty, caller logs `"Unknown 7-Zip error (%d)"`.
pub fn sevenzip_decode_exit_code(ec: i32) -> (bool, &'static CStr) {
    match ec {
        0 => (true, c""),
        1 => (
            false,
            c"Completed with warnings. Some files may have been skipped because they were in use",
        ),
        2 => (
            false,
            c"A fatal error occurred. Check for permission issues, a corrupt archive, password, disk space",
        ),
        7 => (false, c"Command line error"),
        8 => (false, c"Not enough memory for operation"),
        255 => (false, c"User stopped the process"),
        _ => (false, c""),
    }
}

// ============================================================================
// Service & DiskService (`daemon/util/Service.cpp`, `daemon/main/DiskService.cpp`)
// ============================================================================

/// Matches the readiness check in `ServiceCoordinator::Run`:
/// `serviceInterval >= Service::Now && abs(curTime - lastWork) >= serviceInterval`.
pub fn service_should_work_now(
    service_interval: i32,
    service_now: i32,
    cur_time: i64,
    last_work: i64,
) -> bool {
    if service_interval < service_now {
        return false;
    }
    let diff = cur_time.wrapping_sub(last_work).wrapping_abs();
    diff >= i64::from(service_interval)
}

/// Matches the remaining wait calculation in `ServiceCoordinator::Run`:
/// Returns `(remaining, next_wait_interval)`.
pub fn service_calc_remaining_wait(
    service_interval: i32,
    service_now: i32,
    service_sleep: i32,
    cur_time: i64,
    last_work: i64,
    cur_wait_interval: i32,
) -> (i32, i32) {
    let mut remaining = service_sleep;
    let mut wait_interval = cur_wait_interval;
    if service_interval >= service_now {
        let elapsed = cur_time.wrapping_sub(last_work) as i32;
        remaining = service_interval.min(service_interval.wrapping_sub(elapsed));
        wait_interval = wait_interval.min(remaining);
    }
    (remaining, wait_interval)
}

/// Matches `DiskService::ServiceInterval()`.
pub fn disk_service_interval(
    waiting_required_dir: bool,
    disk_space_mb: i32,
    downloading: bool,
    sleep_val: i32,
) -> i32 {
    if waiting_required_dir {
        1
    } else if disk_space_mb <= 0 {
        sleep_val
    } else if !downloading {
        10
    } else {
        1
    }
}

/// Matches the free space check in `DiskService::CheckDiskSpace()`:
/// `disk_space_mb >= 0 && (available_bytes / 1024 / 1024) < disk_space_mb`.
pub fn disk_service_is_low_space(available_bytes: i64, disk_space_mb: i32) -> bool {
    if disk_space_mb < 0 {
        return false;
    }
    (available_bytes / 1024 / 1024) < i64::from(disk_space_mb)
}

// ============================================================================
// NServFrontend (`daemon/nserv/NServFrontend.cpp`)
// ============================================================================

/// Matches the start index and skip check in `NServFrontend::Update()`:
/// `int start = needed - first + 1; if (start < 0) { need_skip = true; start = 0; }`.
pub fn nserv_frontend_calc_start_index(needed_first_id: i32, first_msg_id: i32) -> (i32, bool) {
    let start = needed_first_id.wrapping_sub(first_msg_id).wrapping_add(1);
    if start < 0 {
        (0, true)
    } else {
        (start, false)
    }
}

// ============================================================================
// Extension Scripts (`CommandScript.cpp`, `SchedulerScript.cpp`, `NzbScript.cpp`, `FeedScript.cpp`)
// ============================================================================

/// Matches `CommandScriptController::ExecuteScript` exit code switch:
/// - `0`: `COMMANDPROCESS_SUCCESS` (`93`)
/// - `1`: `COMMANDPROCESS_ERROR` (`94`) or `-1`
/// - `2`: unknown status
pub fn cmd_script_classify_exit_code(exit_code: i32) -> i32 {
    match exit_code {
        93 => 0,
        94 | -1 => 1,
        _ => 2,
    }
}

/// Matches `if (char* ext = strrchr(logPrefix, '.')) *ext = '\0';` in
/// `SchedulerScriptController::ExecuteExternalProcess`.
pub fn sched_script_strip_ext_len(base_name: &[u8]) -> usize {
    match base_name.iter().rposition(|&b| b == b'.') {
        Some(pos) => pos,
        None => base_name.len(),
    }
}

/// Matches the prefix matching logic in `NzbScriptController::PrepareEnvParameters`:
/// - When `strip_prefix` is `None`: returns `Some(0)` (process parameter without stripping).
/// - When `strip_prefix` is `Some(prefix)`: if `param_name` starts with `prefix` and
///   `param_name.len() > prefix.len()`, returns `Some(prefix.len())`, else `None`.
pub fn nzb_script_match_param_prefix(
    param_name: &[u8],
    strip_prefix: Option<&[u8]>,
) -> Option<usize> {
    match strip_prefix {
        None => Some(0),
        Some(prefix) => {
            if param_name.len() > prefix.len() && param_name.starts_with(prefix) {
                Some(prefix.len())
            } else {
                None
            }
        }
    }
}

/// Matches `exitCode == FEED_SUCCESS` (`93`) in `FeedScriptController::ExecuteScript`.
pub fn feed_script_is_success(exit_code: i32) -> bool {
    exit_code == 93
}

// ============================================================================
// SystemHealth Validators (`IncomingNzbValidator`, `UnpackValidator`, `ExtensionScriptsValidator`,
// `NewsServersValidator`, `DisplayValidator`, `SchedulerTaskValidator`, `CategoryValidator`,
// `Status`)
// ============================================================================

/// Matches `NzbDirIntervalValidator::Validate()`:
/// - `0`: Ok (`interval >= 3`)
/// - `1`: Error (`interval < 0`)
/// - `2`: Warning (`interval == 0` -> disabled)
/// - `3`: Warning (`0 < interval < 3` -> very frequent scanning)
pub fn health_validate_nzb_dir_interval(interval: i32) -> i32 {
    if interval < 0 {
        1
    } else if interval == 0 {
        2
    } else if interval < 3 {
        3
    } else {
        0
    }
}

/// Matches `NzbDirFileAgeValidator::Validate()`:
/// - `0`: Ok (`0 <= age <= 3600`)
/// - `1`: Error (`age < 0`)
/// - `2`: Warning (`age > 3600`)
pub fn health_validate_nzb_dir_file_age(age: i32) -> i32 {
    if age < 0 {
        1
    } else if age > 3600 {
        2
    } else {
        0
    }
}

/// Matches `DupeSearchValidator::Validate()`:
/// - `0`: Ok
/// - `1`: Warning (`dupe_search && !dupe_check`)
/// - `2`: Warning (`dupe_search && dupe_check && !health_check_is_dupe`)
pub fn health_validate_dupe_search(
    dupe_search: bool,
    dupe_check: bool,
    health_check_is_dupe: bool,
) -> i32 {
    if !dupe_search {
        return 0;
    }
    if !dupe_check {
        return 1;
    }
    if !health_check_is_dupe {
        return 2;
    }
    0
}

/// Matches `ExtensionCleanupValidator::Validate` and `UnpackIgnoreExtValidator::Validate`:
/// returns `true` when `len <= 512` (Ok), `false` when `len > 512` (Warning).
pub fn health_validate_ext_list_len(len: usize) -> bool {
    len <= 512
}

/// Matches `ExtensionListValidator::Validate` script extension stripping:
/// `size_t dotPos = scriptName.find_last_of('.'); if (dotPos != npos && dotPos > 0) resize(dotPos);`
pub fn health_strip_script_ext_len(name: &[u8]) -> usize {
    match name.iter().rposition(|&b| b == b'.') {
        Some(pos) if pos > 0 => pos,
        _ => name.len(),
    }
}

/// Matches `EventIntervalValidator::Validate()`:
/// returns `true` if `val >= -1` (Ok), `false` if `val < -1` (Error).
pub fn health_validate_event_interval(val: i32) -> bool {
    val >= -1
}

/// Matches `AnyPrimaryServerExistsValidator::Validate()`:
/// - `0`: Ok (at least one server at `minLevel` is active)
/// - `1`: Error (`"At least one server must be configured"`, when empty)
/// - `2`: Error (`"At least one primary server must be active"`)
pub fn health_validate_primary_server(levels: &[i32], actives: &[u8]) -> i32 {
    let n = levels.len().min(actives.len());
    if n == 0 {
        return 1;
    }
    let mut min_level = levels[0];
    for &lvl in &levels[..n] {
        if lvl < min_level {
            min_level = lvl;
        }
    }
    let any_primary_active = (0..n).any(|i| levels[i] == min_level && actives[i] != 0);
    if any_primary_active {
        0
    } else {
        2
    }
}

/// Matches `UpdateIntervalValidator::Validate()`:
/// returns `true` if `v == 0 || v >= 25`, `false` if `v != 0 && v < 25`.
pub fn health_validate_update_interval(v: i32) -> bool {
    v == 0 || v >= 25
}

/// Matches `TimeValidator::Validate()`:
/// - `0`: Ok
/// - `1`: Error (`h < -1 || h > 23`)
/// - `2`: Error (`m < 0 || m > 59`)
pub fn health_validate_scheduler_time(h: i32, m: i32) -> i32 {
    if !(-1..=23).contains(&h) {
        1
    } else if !(0..=59).contains(&m) {
        2
    } else {
        0
    }
}

/// Matches `WeekDaysValidator::Validate()`:
/// returns `true` if `0 <= bits <= 127`.
pub fn health_validate_scheduler_weekdays(bits: i32) -> bool {
    (0..=127).contains(&bits)
}

/// Matches `Category::UnpackValidator::Validate()`:
/// - `0`: Ok (`global_unpack == cat_unpack`)
/// - `1`: Info (`global_unpack && !cat_unpack`)
/// - `2`: Info (`!global_unpack && cat_unpack`)
pub fn health_validate_category_unpack(global_unpack: bool, cat_unpack: bool) -> i32 {
    if global_unpack && !cat_unpack {
        1
    } else if !global_unpack && cat_unpack {
        2
    } else {
        0
    }
}

/// Matches `SystemHealth::SeverityToStr(severity)`:
/// `0 -> "Ok"`, `1 -> "Info"`, `2 -> "Warning"`, `3 -> "Error"`.
pub fn health_severity_to_str(severity: i32) -> &'static CStr {
    match severity {
        0 => c"Ok",
        1 => c"Info",
        2 => c"Warning",
        3 => c"Error",
        _ => c"Ok",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_analytics_and_benchmark() {
        let mut data = [40.0, 10.0, 30.0, 20.0, 50.0];
        let (min, max, avg, med, p25, p75, p90) = data_analytics_compute(&mut data).unwrap();
        assert_eq!(min, 10.0);
        assert_eq!(max, 50.0);
        assert_eq!(avg, 30.0);
        assert_eq!(med, 30.0);
        assert_eq!(p25, 20.0);
        assert_eq!(p75, 40.0);
        assert!((p90 - 46.0).abs() < 1e-9);

        assert_eq!(benchmark_limit_by_free_space(1000, 500, 600), 0);
        assert_eq!(benchmark_limit_by_free_space(1000, 1500, 600), 900);
        assert_eq!(benchmark_calc_write_chunk(4096, 10000, 8000), 2000);
    }

    #[test]
    fn test_unrar_sevenzip_and_services() {
        assert_eq!(unrar_overwrite_switch(0).to_bytes(), b"-o-");
        assert_eq!(unrar_overwrite_switch(1).to_bytes(), b"-o+");
        assert_eq!(unrar_overwrite_switch(2).to_bytes(), b"-or");
        assert!(unrar_is_part1_or_unsplit_rar(b"movie.rar"));
        assert!(unrar_is_part1_or_unsplit_rar(b"movie.part1.rar"));
        assert!(unrar_is_part1_or_unsplit_rar(b"movie.PART001.RAR"));
        assert!(!unrar_is_part1_or_unsplit_rar(b"movie.part2.rar"));
        assert!(!unrar_is_part1_or_unsplit_rar(b"movie.part02.rar"));
        assert!(unrar_decode_exit_code(0).0);
        assert!(!unrar_decode_exit_code(3).0);
        assert!(unrar_decode_exit_code(99).1.to_bytes().is_empty());

        assert_eq!(sevenzip_overwrite_switch(0).to_bytes(), b"-aos");
        assert!(sevenzip_decode_exit_code(0).0);
        assert!(!sevenzip_decode_exit_code(2).0);

        assert!(service_should_work_now(10, 0, 100, 90));
        assert!(!service_should_work_now(10, 0, 95, 90));
        assert_eq!(service_calc_remaining_wait(10, 0, -1, 94, 90, 86400), (6, 6));
        assert_eq!(disk_service_interval(true, 0, false, -1), 1);
        assert_eq!(disk_service_interval(false, 100, false, -1), 10);
        assert!(disk_service_is_low_space(50 * 1024 * 1024, 100));
        assert_eq!(nserv_frontend_calc_start_index(0, 5), (0, true));
        assert_eq!(nserv_frontend_calc_start_index(10, 5), (6, false));
    }

    #[test]
    fn test_scripts_and_health_validators() {
        assert_eq!(cmd_script_classify_exit_code(93), 0);
        assert_eq!(cmd_script_classify_exit_code(94), 1);
        assert_eq!(cmd_script_classify_exit_code(1), 2);
        assert_eq!(sched_script_strip_ext_len(b"myscript.py"), 8);
        assert_eq!(
            nzb_script_match_param_prefix(b"Logger:Verbose", Some(b"Logger:")),
            Some(7)
        );
        assert_eq!(
            nzb_script_match_param_prefix(b"Logger:", Some(b"Logger:")),
            None
        );
        assert!(feed_script_is_success(93));

        assert_eq!(health_validate_nzb_dir_interval(-1), 1);
        assert_eq!(health_validate_nzb_dir_interval(0), 2);
        assert_eq!(health_validate_nzb_dir_interval(2), 3);
        assert_eq!(health_validate_nzb_dir_interval(5), 0);
        assert_eq!(health_validate_nzb_dir_file_age(4000), 2);
        assert_eq!(health_validate_dupe_search(true, false, false), 1);
        assert_eq!(health_validate_dupe_search(true, true, false), 2);
        assert_eq!(health_validate_dupe_search(true, true, true), 0);
        assert!(health_validate_ext_list_len(512));
        assert!(!health_validate_ext_list_len(513));
        assert_eq!(health_strip_script_ext_len(b"QueueSort.py"), 9);
        assert_eq!(health_strip_script_ext_len(b".hidden"), 7);
        assert!(health_validate_event_interval(-1));
        assert!(!health_validate_event_interval(-2));
        assert_eq!(health_validate_primary_server(&[1, 0], &[1, 0]), 2);
        assert_eq!(health_validate_primary_server(&[1, 0], &[0, 1]), 0);
        assert!(health_validate_update_interval(0));
        assert!(!health_validate_update_interval(10));
        assert_eq!(health_validate_scheduler_time(24, 0), 1);
        assert_eq!(health_validate_scheduler_time(12, 60), 2);
        assert_eq!(health_validate_scheduler_time(-1, 30), 0);
        assert!(health_validate_scheduler_weekdays(127));
        assert!(!health_validate_scheduler_weekdays(128));
        assert_eq!(health_validate_category_unpack(true, false), 1);
        assert_eq!(health_validate_category_unpack(false, true), 2);
        assert_eq!(health_severity_to_str(2).to_bytes(), b"Warning");
    }
}
