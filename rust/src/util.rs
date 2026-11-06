//! Util's string helpers: FormatSize, FormatSpeed, AlphaNum, HashBJ96,
//! ReduceStr, the Tokenizer's tokens and MatchFileExt. Each matches the C++
//! code it replaces, quirks included.

use crate::wildmask::Lower;
use std::ffi::{c_char, c_int, CStr};

// Use the process C runtime, just as CString::Format does. Rust's formatter
// ignores LC_NUMERIC and the C floating-point rounding mode. Keep raw bytes:
// a locale's decimal separator need not be UTF-8.
extern "C" {
    fn snprintf(buf: *mut c_char, size: usize, format: *const c_char, ...) -> c_int;
    fn sscanf(s: *const c_char, format: *const c_char, ...) -> c_int;
    fn isspace(c: c_int) -> c_int;
    fn isalpha(c: c_int) -> c_int;
    fn isdigit(c: c_int) -> c_int;
    fn tolower(c: c_int) -> c_int;
    fn atoi(s: *const c_char) -> c_int;
    fn strtod(nptr: *const c_char, endptr: *mut *mut c_char) -> f64;
    fn strtoll(
        nptr: *const c_char,
        endptr: *mut *mut c_char,
        base: c_int,
    ) -> std::ffi::c_longlong;
    fn strtoull(
        nptr: *const c_char,
        endptr: *mut *mut c_char,
        base: c_int,
    ) -> std::ffi::c_ulonglong;
    fn strncasecmp(a: *const c_char, b: *const c_char, n: usize) -> c_int;
    fn strcasecmp(a: *const c_char, b: *const c_char) -> c_int;
}

fn decimal(value: f64, precision: c_int, suffix: &[u8]) -> Vec<u8> {
    let mut buf = vec![0u8; 64];
    loop {
        // The fixed format and promoted varargs types agree; snprintf writes
        // at most buf.len() bytes, including its terminator.
        let n = unsafe { snprintf(buf.as_mut_ptr().cast(), buf.len(), c"%.*f".as_ptr(), precision, value) };
        if n < 0 {
            return Vec::new();
        }
        let n = n as usize;
        if n < buf.len() {
            buf.truncate(n);
            buf.extend_from_slice(suffix);
            return buf;
        }
        buf.resize(n + 1, 0);
    }
}

/// A byte Vec's amortized doubling would exceed its allocation limit.
/// Only a growth request needs intervention; existing spare capacity is used.
pub(crate) fn byte_vec_needs_exact_growth(capacity: usize, len: usize, additional: usize, limit: usize) -> bool {
    additional > capacity.saturating_sub(len) && capacity > limit / 2
}

/// Preserve normal amortized growth, but use the exact required capacity
/// when doubling would exceed the byte Vec's isize::MAX allocation limit.
pub(crate) fn reserve_bytes(out: &mut Vec<u8>, additional: usize) {
    if additional > out.capacity() - out.len() {
        if byte_vec_needs_exact_growth(out.capacity(), out.len(), additional, isize::MAX as usize) {
            out.reserve_exact(additional);
        } else {
            out.reserve(additional);
        }
    }
}

/// `decimal`'s text added to `out`; the value is formatted on the stack.
fn decimal_into(out: &mut Vec<u8>, value: f64, precision: c_int, suffix: &[u8]) {
    let mut stack = [0u8; 64];
    // The fixed format and promoted varargs types agree; snprintf writes at
    // most stack.len() bytes, including its terminator.
    let n = unsafe { snprintf(stack.as_mut_ptr().cast(), stack.len(), c"%.*f".as_ptr(), precision, value) };
    if n < 0 {
        return;
    }
    let n = n as usize;
    reserve_bytes(out, n.saturating_add(suffix.len()));
    if n < stack.len() {
        out.extend_from_slice(&stack[..n]);
    } else {
        let mut buf = vec![0u8; n + 1];
        // the same call into a buffer of the length it asked for
        unsafe { snprintf(buf.as_mut_ptr().cast(), buf.len(), c"%.*f".as_ptr(), precision, value) };
        out.extend_from_slice(&buf[..n]);
    }
    out.extend_from_slice(suffix);
}

/// Util::FormatSize: the C++ computes in float and prints with "%.2f".
pub fn format_size(size: i64) -> Vec<u8> {
    let f = size as f32;
    if size > 1024 * 1024 * 1000 {
        decimal((f / 1024.0 / 1024.0 / 1024.0) as f64, 2, b" GB")
    } else if size > 1024 * 1000 {
        decimal((f / 1024.0 / 1024.0) as f64, 2, b" MB")
    } else if size > 1000 {
        decimal((f / 1024.0) as f64, 2, b" KB")
    } else if size == 0 {
        b"0 MB".to_vec()
    } else {
        // (int) of an int64: wraps
        format!("{} B", size as i32).into_bytes()
    }
}

/// `format_size`'s text added to `out`.
pub fn format_size_into(out: &mut Vec<u8>, size: i64) {
    let f = size as f32;
    if size > 1024 * 1024 * 1000 {
        decimal_into(out, (f / 1024.0 / 1024.0 / 1024.0) as f64, 2, b" GB")
    } else if size > 1024 * 1000 {
        decimal_into(out, (f / 1024.0 / 1024.0) as f64, 2, b" MB")
    } else if size > 1000 {
        decimal_into(out, (f / 1024.0) as f64, 2, b" KB")
    } else if size == 0 {
        reserve_bytes(out, 4);
        out.extend_from_slice(b"0 MB")
    } else {
        // (int) of an int64: wraps
        let text = format!("{} B", size as i32);
        reserve_bytes(out, text.len());
        out.extend_from_slice(text.as_bytes())
    }
}

/// Util::FormatSpeed.
pub fn format_speed(bps: i64) -> Vec<u8> {
    const K: i64 = 1024;
    let d = bps as f64;
    if bps >= 100 * K * K * K {
        format!("{} GB/s", bps / K / K / K).into_bytes()
    } else if bps >= 10 * K * K * K {
        decimal(d / 1024.0 / 1024.0 / 1024.0, 1, b" GB/s")
    } else if bps >= K * K * K {
        decimal(d / 1024.0 / 1024.0 / 1024.0, 2, b" GB/s")
    } else if bps >= 100 * K * K {
        format!("{} MB/s", bps / K / K).into_bytes()
    } else if bps >= 10 * K * K {
        decimal(d / 1024.0 / 1024.0, 1, b" MB/s")
    } else if bps >= K * 1000 {
        decimal(d / 1024.0 / 1024.0, 2, b" MB/s")
    } else {
        format!("{} KB/s", bps / K).into_bytes()
    }
}

/// Util::AlphaNum: only ASCII letters and digits.
pub fn alpha_num(s: &[u8]) -> bool {
    s.iter().all(|b| b.is_ascii_alphanumeric())
}

/// Util::HashBJ96: Bob Jenkins' lookup2 hash.
pub fn hash_bj96(k: &[u8], init: u32) -> u32 {
    fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
        *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 13);
        *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 8);
        *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 13);
        *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 12);
        *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 16);
        *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 5);
        *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 3);
        *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 10);
        *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 15);
    }
    let word = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    let (mut a, mut b, mut c) = (0x9e37_79b9u32, 0x9e37_79b9u32, init);
    // the whole length, as the C++ uint32 had it (the caller passes an int)
    let length = k.len() as u32;
    let mut k = k;
    while k.len() >= 12 {
        a = a.wrapping_add(word(&k[0..4]));
        b = b.wrapping_add(word(&k[4..8]));
        c = c.wrapping_add(word(&k[8..12]));
        mix(&mut a, &mut b, &mut c);
        k = &k[12..];
    }
    c = c.wrapping_add(length);
    let mut tail = [0u8; 12];
    tail[..k.len()].copy_from_slice(k);
    a = a.wrapping_add(word(&tail[0..4]));
    b = b.wrapping_add(word(&tail[4..8]));
    // the first byte of c is reserved for the length
    c = c.wrapping_add(word(&tail[8..12]) << 8);
    mix(&mut a, &mut b, &mut c);
    c
}

/// Util::ReduceStr, preserving the legacy forward copies, including every
/// intermediate NUL and the truncation caused by equal-length replacements.
/// Read operands afresh: they may point into the buffer being modified.
///
/// # Safety
/// `s` is null or a writable NUL-terminated string. `from` and `to` are null
/// or readable NUL-terminated strings, and may alias `s`. As in C++, operands
/// must stay terminated during the call. Empty patterns and growing
/// replacements (which could loop forever or overflow in C++) are no-ops.
pub unsafe fn reduce_str(s: *mut c_char, from: *const c_char, to: *const c_char) {
    if s.is_null() || from.is_null() || to.is_null() {
        return;
    }
    let capacity = CStr::from_ptr(s).to_bytes().len();
    let len_from = CStr::from_ptr(from).to_bytes().len();
    let len_to = CStr::from_ptr(to).to_bytes().len();
    if len_from == 0 || len_to > len_from {
        return;
    }
    loop {
        // End all shared borrows before writing; an operand may alias s.
        let text = CStr::from_ptr(s).to_bytes();
        let pattern = CStr::from_ptr(from).to_bytes();
        if pattern.is_empty() {
            return;
        }
        let Some(p) = text.windows(pattern.len()).position(|w| w == pattern) else { return };
        // Aliasing can change either operand's length. The C++ still uses
        // their initial lengths to locate the tail, even past the current
        // NUL. Bound access by the original allocation's known string span.
        let replacement_len = CStr::from_ptr(to).to_bytes().len();
        let shift = len_from - len_to;
        if shift > capacity - p || replacement_len > capacity - p - shift {
            return;
        }
        for i in 0..=replacement_len {
            let byte = *to.add(i);
            *s.add(p + i) = byte;
            // A forward-overlapping copy that overwrote its own terminator
            // would run off the allocation in C++; stop before doing that.
            if i == replacement_len && byte != 0 { return; }
        }
        let mut dest = p + replacement_len;
        let mut source = dest + shift;
        loop {
            let byte = *s.add(source);
            *s.add(dest) = byte;
            if byte == 0 { break; }
            dest += 1;
            source += 1;
        }
    }
}

/// The tokens of the C++ Tokenizer: strtok_r's (runs between separators),
/// trimmed of spaces, tabs, CR and LF, empty ones skipped.
pub fn tokens<'a>(s: &'a [u8], separators: &'a [u8]) -> impl Iterator<Item = &'a [u8]> + 'a {
    s.split(move |b| separators.contains(b)).filter_map(|t| {
        let is_ws = |b: &u8| matches!(b, b'\n' | b'\r' | b' ' | b'\t');
        let start = t.iter().position(|b| !is_ws(b))?;
        let end = t.iter().rposition(|b| !is_ws(b))? + 1;
        Some(&t[start..end])
    })
}

/// Util::MatchFileExt: whether `filename` ends with one of the extensions of
/// `list` (strcasecmp, folding through `case_lower`, unsigned bytes) or matches
/// one with '*' or '?' as a WildMask (folding through `mask_lower`, as the C++
/// char).
pub fn match_file_ext(filename: &[u8], list: &[u8], separators: &[u8], case_lower: &Lower<'_>, mask_lower: &Lower<'_>) -> bool {
    tokens(list, separators).any(|ext| {
        (filename.len() >= ext.len()
            && filename[filename.len() - ext.len()..].iter().zip(ext).all(|(&a, &b)| case_lower.eq(a, b)))
            || (ext.iter().any(|&b| b == b'*' || b == b'?')
                && crate::wildmask::wild_match(mask_lower, ext, filename, None))
    })
}

/// Util::SplitCommandLine: words split at spaces; a word starting with `'`
/// runs to the next lone `'` (`''` is a quote), each cut to 1023 bytes.
pub fn split_command_line(s: &[u8]) -> Vec<Vec<u8>> {
    const MAX: usize = 1023;
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut result = Vec::new();
    let mut buf = Vec::new();
    let mut escaping = false;
    let mut space = true;
    let mut i = 0;
    loop {
        let c = at(i);
        if c != 0 {
            if escaping {
                if c == b'\'' {
                    if at(i + 1) == b'\'' && buf.len() < MAX {
                        buf.push(c);
                        i += 1;
                    } else {
                        escaping = false;
                        space = true;
                    }
                } else if buf.len() < MAX {
                    buf.push(c);
                }
            } else if c == b' ' {
                space = true;
            } else if c == b'\'' && space {
                escaping = true;
                space = false;
            } else if buf.len() < MAX {
                buf.push(c);
                space = false;
            }
        }
        if (space || c == 0) && !buf.is_empty() {
            result.push(std::mem::take(&mut buf));
        }
        if c == 0 {
            break;
        }
        i += 1;
    }
    result
}

fn line_space(c: u8) -> bool {
    matches!(c, b'\n' | b'\r' | b' ' | b'\t')
}

/// Util::TrimRight(char*): the length without trailing CR, LF, space and tab.
pub fn trim_right_line(s: &[u8]) -> usize {
    s.len() - s.iter().rev().take_while(|&&c| line_space(c)).count()
}

/// Util::Trim(char*): (start, end) without CR, LF, space and tab around.
pub fn trim_line(s: &[u8]) -> (usize, usize) {
    let end = trim_right_line(s);
    (s[..end].iter().take_while(|&&c| line_space(c)).count(), end)
}

/// Util::Trim(std::string&): (start, end) without the locale's isspace around:
/// TrimLeft tests unsigned bytes; the caller classifies TrimRight's C++ char
/// (whose signedness can differ from Rust's c_char with compiler flags).
pub fn trim_string(s: &[u8], left: bool, right: bool, right_space: impl Fn(u8) -> bool) -> (usize, usize) {
    let start = if left { s.iter().take_while(|&&c| unsafe { isspace(c as c_int) } != 0).count() } else { 0 };
    let mut end = s.len();
    if right {
        while end > start && right_space(s[end - 1]) {
            end -= 1;
        }
        // TrimRight runs after TrimLeft erased the start: nothing more to strip
    }
    (start, end)
}

/// Util::SanitizeLine: control characters (below space, and DEL) become
/// spaces, then Trim.
pub fn sanitize_line(s: &mut [u8], right_space: impl Fn(u8) -> bool) -> (usize, usize) {
    for c in s.iter_mut() {
        if *c < 32 || *c == 127 {
            *c = b' ';
        }
    }
    trim_string(s, true, true, right_space)
}

/// Util::EndsWith (std::string_view): `suffix` ends `s`, case-insensitively
/// with the locale's tolower of unsigned bytes unless `case_sensitive`.
pub fn ends_with(s: &[u8], suffix: &[u8], case_sensitive: bool) -> bool {
    if suffix.is_empty() {
        return true;
    }
    if s.len() < suffix.len() {
        return false;
    }
    let tail = &s[s.len() - suffix.len()..];
    if case_sensitive {
        return tail == suffix;
    }
    tail.iter().zip(suffix).all(|(&a, &b)| unsafe { tolower(a as c_int) == tolower(b as c_int) })
}

/// Util::FormatBuffer: each byte as `%02x `.
pub fn format_buffer(buf: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(buf.len() * 3 + 1);
    for &b in buf {
        out.extend_from_slice(&[HEX[(b >> 4) as usize], HEX[(b & 15) as usize], b' ']);
    }
    out
}

/// Util::Timegm (Boost's arithmetic) on the fields ParseRfc822DateTime sets,
/// in the C++ int arithmetic (wrapping where it would overflow).
fn timegm_c(year: i32, mon: i32, mday: i32, hour: i32, min: i32, sec: i32) -> i64 {
    const DAYS: [[i32; 12]; 2] = [
        [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334],
        [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335],
    ];
    let leap = year % 400 == 0 || (year % 100 != 0 && year % 4 == 0);
    let y = year.wrapping_sub(1);
    let days_from_0 = 365i32.wrapping_mul(y).wrapping_add(y / 400).wrapping_sub(y / 100).wrapping_add(y / 4);
    let day_of_year = DAYS[leap as usize][mon as usize].wrapping_add(mday).wrapping_sub(1);
    let days = days_from_0.wrapping_sub(719162).wrapping_add(day_of_year);
    86400i64 * days as i64 + 3600i32.wrapping_mul(hour) as i64 + 60i32.wrapping_mul(min) as i64 + sec as i64
}

/// WebUtil::ParseRfc822DateTime: `[Day,] DD Mon YYYY HH:MM[:SS] [zone]` as a
/// Unix time, 0 if it doesn't parse. Numbers are read by the C library
/// (sscanf, atoi) and names compared in the current locale, as before.
pub fn parse_rfc822_date_time(s: &CStr) -> i64 {
    let bytes = s.to_bytes_with_nul();
    let mut p = 0;
    while bytes[p] == b' ' {
        p += 1;
    }
    if unsafe { isalpha(bytes[p] as c_int) } != 0 {
        while bytes[p] != 0 && bytes[p] != b',' && bytes[p] != b' ' {
            p += 1;
        }
        if bytes[p] == b',' {
            p += 1;
        }
    }
    let at = |p: usize| bytes[p..].as_ptr().cast::<c_char>();
    let mut month = [0 as c_char; 4];
    let (mut day, mut year, mut hours, mut minutes, mut seconds, mut len): (c_int, c_int, c_int, c_int, c_int, c_int) =
        (0, 0, 0, 0, 0, 0);
    // the format's conversions and the argument types agree; %3s writes at most 4 bytes
    let n = unsafe {
        sscanf(
            at(p),
            c"%d %3s %d %d:%d%n".as_ptr(),
            &mut day as *mut c_int,
            month.as_mut_ptr(),
            &mut year as *mut c_int,
            &mut hours as *mut c_int,
            &mut minutes as *mut c_int,
            &mut len as *mut c_int,
        )
    };
    if n != 5 {
        return 0;
    }
    p += len as usize;
    if bytes[p] == b':' {
        let mut sec_len: c_int = 0;
        if unsafe { sscanf(at(p + 1), c"%d%n".as_ptr(), &mut seconds as *mut c_int, &mut sec_len as *mut c_int) } != 1 {
            return 0;
        }
        p += 1 + sec_len as usize;
    }
    while bytes[p] == b' ' {
        p += 1;
    }
    let mut zone_offset: i32 = 0; // minutes east of UTC
    if (bytes[p] == b'+' || bytes[p] == b'-') && unsafe { isdigit(bytes[p + 1] as c_int) } != 0 {
        let zone = unsafe { atoi(at(p + 1)) };
        zone_offset = (zone / 100).wrapping_mul(60).wrapping_add(zone % 100);
        if bytes[p] == b'-' {
            zone_offset = zone_offset.wrapping_neg();
        }
    } else {
        const ZONES: [(&CStr, i32); 8] = [
            (c"EST", -5), (c"EDT", -4), (c"CST", -6), (c"CDT", -5),
            (c"MST", -7), (c"MDT", -6), (c"PST", -8), (c"PDT", -7),
        ];
        if let Some((_, hours)) = ZONES.iter().find(|(name, _)| unsafe { strncasecmp(at(p), name.as_ptr(), 3) } == 0) {
            zone_offset = hours * 60;
        }
    }
    const MONTHS: [&CStr; 12] = [c"Jan", c"Feb", c"Mar", c"Apr", c"May", c"Jun", c"Jul", c"Aug", c"Sep", c"Oct", c"Nov", c"Dec"];
    let mon = MONTHS.iter().position(|m| unsafe { strcasecmp(month.as_ptr(), m.as_ptr()) } == 0).unwrap_or(0) as i32;
    timegm_c(year, mon, day, hours, minutes, seconds) - zone_offset.wrapping_mul(60) as i64
}

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

/// Matches `DiskBenchmark::LimitByFreeSpace(requested, available)`.
pub fn benchmark_limit_by_free_space(requested: u64, available: u64, reserve: u64) -> u64 {
    if available <= reserve {
        0
    } else {
        requested.min(available - reserve)
    }
}

/// Matches the per-iteration write chunk calculation in `DiskBenchmark::Run`.
pub fn benchmark_calc_write_chunk(block_size: usize, max_bytes: u64, write_bytes: u64) -> usize {
    if write_bytes >= max_bytes {
        return 0;
    }
    let rem = max_bytes - write_bytes;
    (block_size as u64).min(rem) as usize
}

/// Matches the readiness check in `ServiceCoordinator::Run`.
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

/// Matches the free space check in `DiskService::CheckDiskSpace()`.
pub fn disk_service_is_low_space(available_bytes: i64, disk_space_mb: i32) -> bool {
    if disk_space_mb < 0 {
        return false;
    }
    (available_bytes / 1024 / 1024) < i64::from(disk_space_mb)
}

/// Joins two 32-bit unsigned integers (`hi`, `lo`) into a 64-bit signed integer (`Util::JoinInt64`).
pub fn util_join_int64(hi: u32, lo: u32) -> i64 {
    ((hi as i64) << 32) | (lo as i64)
}

/// Splits a 64-bit signed integer into `(hi, lo)` 32-bit unsigned integers (`Util::SplitInt64`).
pub fn util_split_int64(int64_val: i64) -> (u32, u32) {
    let u = int64_val as u64;
    ((u >> 32) as u32, (u & 0xFFFF_FFFF) as u32)
}

/// Matches a `Tokenizer` token `shellover` (of the form `ext=cmd`) against
/// `file_ext`, returning `Some(cmd_offset)` when `file_ext` equals the prefix
/// before `'='` (`Util::FindShellOverriddenExecutor`).
pub fn util_match_shell_overridden_token(shellover: &[u8], file_ext: &[u8]) -> Option<usize> {
    let eq_pos = shellover.iter().position(|&b| b == b'=')?;
    if &shellover[..eq_pos] == file_ext {
        Some(eq_pos + 1)
    } else {
        None
    }
}

/// Resolves `path` against colon-separated directories in `env_path`, returning the
/// first directory entry where `dir/path` exists and is a regular file (`Util::ResolvePathFromEnv`).
pub fn util_resolve_path_from_env(path: &[u8], env_path: &[u8]) -> Option<Vec<u8>> {
    if path.is_empty() || env_path.is_empty() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let rel = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
        for dir in env_path.split(|&b| b == b':') {
            if dir.is_empty() {
                continue;
            }
            let base = std::path::Path::new(std::ffi::OsStr::from_bytes(dir));
            let candidate = base.join(rel);
            if candidate.exists() && candidate.is_file() {
                return Some(candidate.as_os_str().as_bytes().to_vec());
            }
        }
    }
    None
}

/// Converts a broken-down UTC `tm` tuple into seconds since the Unix epoch (`Util::Timegm`).
pub fn util_internal_timegm(
    tm_year: i32,
    tm_mon: i32,
    tm_mday: i32,
    tm_hour: i32,
    tm_min: i32,
    tm_sec: i32,
) -> i64 {
    let mut year = tm_year.wrapping_add(1900);
    let mut month = tm_mon;
    if month > 11 {
        year = year.wrapping_add(month / 12);
        month %= 12;
    } else if month < 0 {
        let years_diff = month.wrapping_neg().wrapping_add(11) / 12;
        year = year.wrapping_sub(years_diff);
        month = month.wrapping_add(12_i32.wrapping_mul(years_diff));
    }
    timegm_c(year, month.clamp(0, 11), tm_mday, tm_hour, tm_min, tm_sec)
}

/// Computes the bounded copy length for an XML tag value into a buffer of size
/// `value_buf_size` (`WebUtil::XmlParseTagValue`).
pub fn webutil_xml_calc_copy_len(value_len: i32, value_buf_size: i32) -> i32 {
    if value_len < value_buf_size {
        value_len
    } else {
        value_buf_size.wrapping_sub(1)
    }
}

/// Computes the match length `rm_eo - rm_so` for a regex capture group (`RegEx::GetMatchLen`).
pub fn regex_calc_match_len(rm_so: i32, rm_eo: i32) -> i32 {
    rm_eo.wrapping_sub(rm_so)
}

/// Returns `1` when a tokenizer input string of length `len` fits in the inline
/// short-string buffer of capacity `capacity` (`Tokenizer::Tokenizer`).
pub fn tokenizer_fits_short_string(len: i32, capacity: i32) -> i32 {
    i32::from(len < capacity)
}

/// Validates a file size for `FileSystem::LoadFileIntoBuffer` (`0 <= file_size <= INT_MAX - 1`)
/// and returns `Some((size, buf_size))`.
pub fn fs_validate_load_file_size(file_size: i64, add_trailing_null: i32) -> Option<(i32, i32)> {
    if file_size < 0 || file_size > (i32::MAX as i64) - 1 {
        return None;
    }
    let size = file_size as i32;
    let buf_size = size + if add_trailing_null != 0 { 1 } else { 0 };
    Some((size, buf_size))
}

/// Computes the next write chunk size (`min(remaining, zeros_size)`) when
/// pre-allocating a file with zeroes (`FileSystem::AllocateFile`).
pub fn fs_calc_allocate_chunk_size(remaining: i64, zeros_size: i64) -> i64 {
    remaining.min(zeros_size)
}

/// Classifies whether `filename` requires relative-to-cwd expansion (`1` if non-empty
/// and not starting with `'/'`, `0` otherwise) and returns the leading `"./"` skip offset.
pub fn fs_classify_expand_file_name(filename: &[u8]) -> (i32, usize) {
    if filename.is_empty() || filename[0] == b'/' {
        return (0, 0);
    }
    let skip = if filename.len() >= 2 && filename[0] == b'.' && filename[1] == b'/' {
        2
    } else {
        0
    };
    (1, skip)
}

/// Returns `1` when `filename` is `"."` or `".."`, `0` otherwise (`DirBrowser::Next`).
pub fn dir_browser_is_dot_or_dotdot(filename: &[u8]) -> i32 {
    i32::from(filename == b"." || filename == b"..")
}

/// Maps `DiskFile::ESeekOrigin` to the C stdio `SEEK_*` origin constant (`DiskFile::Seek`).
pub fn disk_file_select_seek_origin(
    origin: i32,
    so_cur: i32,
    so_end: i32,
    seek_set: i32,
    seek_cur: i32,
    seek_end: i32,
) -> i32 {
    if origin == so_cur {
        seek_cur
    } else if origin == so_end {
        seek_end
    } else {
        seek_set
    }
}

/// Returns `1` when `opt_name` starts with `strip_prefix` (of length `prefix_len`)
/// and has at least one character after the prefix (`ScriptController::PrepareEnvOptions`).
pub fn script_ctrl_matches_env_prefix(
    opt_name: &[u8],
    strip_prefix: Option<&[u8]>,
    prefix_len: i32,
) -> i32 {
    let Some(prefix) = strip_prefix else {
        return 0;
    };
    if prefix_len < 0 {
        return 0;
    }
    let plen = prefix_len as usize;
    let cmp_prefix = if plen <= prefix.len() {
        &prefix[..plen]
    } else {
        prefix
    };
    i32::from(
        opt_name.len() > plen
            && opt_name.len() >= cmp_prefix.len()
            && &opt_name[..cmp_prefix.len()] == cmp_prefix,
    )
}

/// Returns `1` when the first output line from a child process begins with
/// `"[ERROR] Could not start "` (`ScriptController::Execute`).
pub fn script_ctrl_is_fork_start_error_line(first_line: i32, buf: &[u8]) -> i32 {
    const PREFIX: &[u8] = b"[ERROR] Could not start ";
    i32::from(first_line != 0 && buf.starts_with(PREFIX))
}

/// Normalizes a child exit code to `-1` when `exit_code == fork_error_exit_code` and
/// `start_error != 0` (`ScriptController::Execute`).
pub fn script_ctrl_normalize_fork_exit_code(
    exit_code: i32,
    fork_error_exit_code: i32,
    start_error: i32,
) -> i32 {
    if exit_code == fork_error_exit_code && start_error != 0 {
        -1
    } else {
        exit_code
    }
}

/// Computes the shell-convention exit code `128 + term_sig` for a process terminated
/// by a signal (`ScriptController::WaitProcess`).
pub fn script_ctrl_calc_signal_exit_code(term_sig: i32) -> i32 {
    128_i32.wrapping_add(term_sig)
}

/// Selects the target PID/PGID argument for `kill()` when terminating a child process (`ScriptController::Terminate`).
pub fn script_ctrl_select_kill_pid(child_pid: i32, child_pgid: i32) -> i32 {
    if child_pid > 0 && child_pgid == child_pid {
        child_pid.wrapping_neg()
    } else {
        child_pid
    }
}

/// `Log::IntervalCheck` & `Log::RotateLog`:
pub fn log_should_close_idle(cur_time: i64, last_written: i64) -> bool {
    cur_time.wrapping_sub(last_written).saturating_abs() > 1
}

pub fn log_should_delete_old_rotated(file_day: i32, cur_day: i32, rotate_log: i32) -> bool {
    file_day <= cur_day.wrapping_sub(rotate_log)
}

/// `BString<size>::Set` & `BString<size>::Append`: computes bounded copy length.
pub fn nstring_bstring_set_len(size: i32, len: i32) -> i32 {
    let cap = size.saturating_sub(1).max(0);
    if len > 0 {
        cap.min(len)
    } else {
        cap
    }
}

pub fn nstring_bstring_append_len(size: i32, cur_len: i32, str_len: i32) -> i32 {
    let avail = size.wrapping_sub(cur_len).wrapping_sub(1);
    avail.min(str_len)
}

/// `CString::Replace(int pos, int len, const char* str, int strLen)`:
/// computes `(valid, add_len, del_len, new_len)` where `valid` is `pos <= cur_len`.
pub fn nstring_cstring_replace_bounds(
    pos: i32,
    len: i32,
    cur_len: i32,
    raw_str_len: i32,
    str_len_param: i32,
) -> (bool, i32, i32, i32) {
    if pos > cur_len {
        return (false, 0, 0, 0);
    }
    let add_len = if str_len_param > 0 {
        raw_str_len.min(str_len_param)
    } else {
        raw_str_len
    };
    let del_len = if pos.wrapping_add(len) <= cur_len {
        len
    } else {
        cur_len.wrapping_sub(pos)
    };
    let new_len = cur_len.wrapping_sub(del_len).wrapping_add(add_len);
    (true, add_len, del_len, new_len)
}

/// `CString::TrimRight`: returns the length of `s` after stripping trailing
/// `'\n'`, `'\r'`, `' '`, and `'\t'` bytes.
pub fn nstring_cstring_trim_right_len(s: &[u8]) -> usize {
    let mut end = s.len();
    while end > 0 {
        let ch = s[end - 1];
        if ch == b'\n' || ch == b'\r' || ch == b' ' || ch == b'\t' {
            end -= 1;
        } else {
            break;
        }
    }
    end
}

/// Evaluates whether `Log::Filelog` should rotate the log file on a day boundary.
pub fn log_should_rotate(rawtime: i64, last_written: i64, write_log_mode: i32) -> bool {
    const DAY: i32 = 86400;
    (rawtime as i32) / DAY != (last_written as i32) / DAY && write_log_mode == 3
}

/// Determines `(write_to_file, keep_in_screen_log)` for a message target in `Log::InitOptions`.
pub fn log_init_message_disposition(target: i32) -> (bool, bool) {
    let write_to_file = target == 2 || target == 3;
    let keep_in_screen = target == 1 || target == 3;
    (write_to_file, keep_in_screen)
}

/// Decodes a UTF-8 byte slice into UTF-16 code units following `WString::WString(const char* utfstr)`.
pub fn wstring_decode_utf8_to_utf16(utf8: &[u8], out: &mut [u16]) -> usize {
    let mut written = 0usize;
    let mut codepoint: u32 = 0;
    let mut i = 0usize;
    while i < utf8.len() && utf8[i] != 0 {
        let ch = utf8[i];
        if ch <= 0x7f {
            codepoint = ch as u32;
        } else if ch <= 0xbf {
            codepoint = (codepoint << 6) | ((ch & 0x3f) as u32);
        } else if ch <= 0xdf {
            codepoint = (ch & 0x1f) as u32;
        } else if ch <= 0xef {
            codepoint = (ch & 0x0f) as u32;
        } else {
            codepoint = (ch & 0x07) as u32;
        }
        i += 1;
        let next_byte = if i < utf8.len() { utf8[i] } else { 0 };
        if (next_byte & 0xc0) != 0x80 && codepoint <= 0x10ffff {
            if codepoint > 0xffff {
                if written + 1 < out.len() {
                    out[written] = (0xd800 + (codepoint >> 10)) as u16;
                    out[written + 1] = (0xdc00 + (codepoint & 0x03ff)) as u16;
                    written += 2;
                }
            } else if !(0xd800..0xe000).contains(&codepoint) && written < out.len() {
                out[written] = codepoint as u16;
                written += 1;
            }
        }
    }
    written
}

/// Computes the target allocation capacity for `StringBuilder::Reserve(int capacity, bool exact)`.
pub fn string_builder_calc_reserve_capacity(
    old_capacity: i32,
    requested_capacity: i32,
    exact: bool,
) -> i32 {
    if requested_capacity > old_capacity || old_capacity == 0 {
        let smart_capacity = if exact {
            0
        } else {
            ((old_capacity as f64) * 1.5) as i32
        };
        if smart_capacity > requested_capacity {
            smart_capacity
        } else {
            requested_capacity
        }
    } else {
        -1
    }
}

/// Formats `"<cur_dir>/<rel_filename>"` for `FileSystem::ExpandFileName`.
pub fn fs_format_expanded_rel_path(cur_dir: &[u8], rel_filename: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(cur_dir.len() + 1 + rel_filename.len());
    out.extend_from_slice(cur_dir);
    out.push(b'/');
    out.extend_from_slice(rel_filename);
    out
}

/// Formats `"could not delete (null): <last_err>"` for `FileSystem::DeleteDirectoryWithContent`.
pub fn fs_format_null_delete_errmsg(last_err: &[u8]) -> Vec<u8> {
    let prefix = b"could not delete (null): ";
    let mut out = Vec::with_capacity(prefix.len() + last_err.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(last_err);
    out
}

/// Extracts the parent directory path up to the last `sep` (or the whole `filename` if no `sep` exists)
/// for `FileSystem::FlushDirBuffers`.
pub fn fs_extract_parent_dir_path(filename: &[u8], sep: u8) -> Vec<u8> {
    match filename.iter().rposition(|&b| b == sep) {
        Some(pos) => filename[..pos].to_vec(),
        None => filename.to_vec(),
    }
}

/// Returns `true` when byte slices `a` and `b` are equal (`strcmp(a, b) == 0`).
pub fn bytes_equal(a: &[u8], b: &[u8]) -> bool {
    a == b
}

/// Returns `true` when byte slices `a` and `b` are equal under `releasename::c_lower` (`strcasecmp(a, b) == 0`)
/// without allocating temporary buffers and stopping at the first mismatch.
pub fn bytes_eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(&x, &y)| {
            x == y || crate::releasename::c_lower(x) == crate::releasename::c_lower(y)
        })
}

/// Parses a leading signed base-10 integer from `text` following C `atoll` / `strtoll(..., 10)`
/// semantics (skips leading ASCII whitespace, optional `+`/`-`, consecutive decimal digits).
pub fn parse_c_atoll(text: &[u8]) -> i64 {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < text.len() {
        if text[i] == b'-' {
            neg = true;
            i += 1;
        } else if text[i] == b'+' {
            i += 1;
        }
    }
    let mut acc: u64 = 0;
    let mut any = false;
    while i < text.len() && text[i].is_ascii_digit() {
        any = true;
        acc = acc
            .saturating_mul(10)
            .saturating_add(u64::from(text[i] - b'0'));
        i += 1;
    }
    if !any {
        return 0;
    }
    if neg {
        let max_neg = (i64::MAX as u64) + 1;
        if acc >= max_neg {
            i64::MIN
        } else {
            -(acc as i64)
        }
    } else {
        acc.min(i64::MAX as u64) as i64
    }
}

/// Parses a leading signed base-10 integer from `text` following C `atoi` semantics.
pub fn parse_c_atoi(text: &[u8]) -> i32 {
    parse_c_atoll(text) as i32
}

/// Parses a leading signed base-10 integer from `text` when `Some`, or returns `default_val` when `None`.
pub fn parse_c_atoi_or_default(text: Option<&[u8]>, default_val: i32) -> i32 {
    match text {
        Some(s) => parse_c_atoi(s),
        None => default_val,
    }
}

/// Finds the first occurrence of `needle` in `haystack` starting at `pos` for `CString::Find`.
pub fn nstring_find_substr_from(haystack: &[u8], needle: &[u8], pos: i32) -> i32 {
    if pos < 0 || (pos != 0 && (pos as usize) >= haystack.len()) {
        return -1;
    }
    let start = pos as usize;
    if needle.is_empty() {
        return pos;
    }
    match haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
    {
        Some(rel) => pos + (rel as i32),
        None => -1,
    }
}

/// Returns `true` when two byte slices are identical.
pub fn mem_slices_equal(a: &[u8], b: &[u8]) -> bool {
    a == b
}

/// Classifies the file extension in `Util::FindExecutorProgram`:
/// returns `(0, 0)` when no `.` is present, or `(kind, dot_idx)` where `kind` is:
/// `1` = `".py"`, `2` = `".sh"`, `3` = `".js"`, `4` = any other extension.
pub fn util_classify_executor_ext(filename: &[u8]) -> (i32, usize) {
    match filename.iter().rposition(|&b| b == b'.') {
        None => (0, 0),
        Some(idx) => {
            let ext = &filename[idx..];
            let kind = match ext {
                b".py" => 1,
                b".sh" => 2,
                b".js" => 3,
                _ => 4,
            };
            (kind, idx)
        }
    }
}

/// Finds the next non-empty trimmed token `(tok_start, tok_end, next_pos)` in `s`
/// separated by any byte in `separators` and trimmed via `crate::util::trim_line`,
/// for `Tokenizer::Next`.
pub fn tokenizer_find_next_token(
    s: &[u8],
    separators: &[u8],
) -> Option<(usize, usize, usize)> {
    let mut pos = 0;
    while pos < s.len() {
        while pos < s.len() && separators.contains(&s[pos]) {
            pos += 1;
        }
        if pos >= s.len() {
            return None;
        }
        let raw_end = pos
            + s[pos..]
                .iter()
                .take_while(|&&b| !separators.contains(&b))
                .count();
        let next_pos = if raw_end < s.len() {
            raw_end + 1
        } else {
            raw_end
        };
        let (trim_start, trim_end) = crate::util::trim_line(&s[pos..raw_end]);
        if trim_start < trim_end {
            return Some((pos + trim_start, pos + trim_end, next_pos));
        }
        pos = next_pos;
    }
    None
}

/// Returns true when `FileSystem::MoveFile` should fall back from `rename` to `CopyFile` + `DeleteFile` (`!rename_ok && err_no == exdev`).
pub fn fs_should_fallback_move_to_copy(rename_ok: bool, err_no: i32, exdev: i32) -> bool {
    !rename_ok && err_no == exdev
}

/// Returns true when `FileSystem::CopyFile` closed cleanly, had no read/write error, and copied the exact source size.
pub fn fs_is_copied_file_valid(
    close_ok: bool,
    loop_ok: bool,
    copied_bytes: i64,
    src_size: i64,
) -> bool {
    close_ok && loop_ok && copied_bytes == src_size
}

// ============================================================================
// DiskBenchmark (`daemon/util/Benchmark.cpp`)
// ============================================================================

pub const BENCHMARK_DEFAULT_BLOCK_SIZE: usize = 1024 * 1024;
pub const BENCHMARK_MAX_BLOCK_SIZE: usize = 512 * 1024 * 1024;
pub const BENCHMARK_FREE_SPACE_RESERVE: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DiskBenchmarkResult {
    pub write_bytes: u64,
    pub write_ms: f64,
    pub read_bytes: u64,
    pub read_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskBenchmarkError {
    InvalidArgument(String),
    Runtime(String),
}

struct TestFileRemover {
    path: std::path::PathBuf,
}

impl Drop for TestFileRemover {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Port of `Benchmark::DiskBenchmark::Run`.
pub fn disk_benchmark_run(
    dir: &str,
    block_size_bytes: usize,
    max_file_size_bytes: u64,
    timeout: std::time::Duration,
    available_space: Result<u64, String>,
) -> Result<DiskBenchmarkResult, DiskBenchmarkError> {
    disk_benchmark_run_bytes(
        dir.as_bytes(),
        block_size_bytes,
        max_file_size_bytes,
        timeout,
        available_space,
    )
}

/// Port of `Benchmark::DiskBenchmark::Run` accepting raw OS path bytes.
pub fn disk_benchmark_run_bytes(
    dir_bytes: &[u8],
    block_size_bytes: usize,
    max_file_size_bytes: u64,
    timeout: std::time::Duration,
    available_space: Result<u64, String>,
) -> Result<DiskBenchmarkResult, DiskBenchmarkError> {
    use std::io::{BufReader, BufWriter, Read, Write};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    if block_size_bytes > BENCHMARK_MAX_BLOCK_SIZE {
        return Err(DiskBenchmarkError::InvalidArgument(
            "The buffer size is too big".to_string(),
        ));
    }
    let block_size = if block_size_bytes == 0 {
        BENCHMARK_DEFAULT_BLOCK_SIZE
    } else {
        block_size_bytes
    };

    #[cfg(unix)]
    let dir_path = {
        use std::os::unix::ffi::OsStrExt;
        std::path::Path::new(std::ffi::OsStr::from_bytes(dir_bytes)).to_path_buf()
    };
    #[cfg(not(unix))]
    let dir_path = std::path::PathBuf::from(String::from_utf8_lossy(dir_bytes).into_owned());

    let dir_display = String::from_utf8_lossy(dir_bytes);
    if !dir_path.is_dir() {
        return Err(DiskBenchmarkError::Runtime(format!(
            "Directory does not exist: {dir_display}"
        )));
    }

    let available = available_space.map_err(|msg| {
        DiskBenchmarkError::Runtime(format!(
            "Could not determine free disk space for {dir_display}: {msg}"
        ))
    })?;
    if available < BENCHMARK_FREE_SPACE_RESERVE.saturating_add(block_size as u64) {
        return Err(DiskBenchmarkError::Runtime(
            "Not enough free disk space for the test".to_string(),
        ));
    }
    let max_bytes =
        benchmark_limit_by_free_space(max_file_size_bytes, available, BENCHMARK_FREE_SPACE_RESERVE);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let unique_name = format!("nzbget_bench_{nanos}.tmp");
    let file_path = dir_path.join(unique_name);

    let mut buffer = vec![0u8; block_size];
    let mut seed = (nanos as u64) ^ 0x9e37_79b9_7f4a_7c15;
    for b in &mut buffer {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *b = seed as u8;
    }

    let _remover = TestFileRemover {
        path: file_path.clone(),
    };
    let mut result = DiskBenchmarkResult::default();

    {
        let file = std::fs::File::create(&file_path).map_err(|_| {
            DiskBenchmarkError::Runtime("Failed to create test file".to_string())
        })?;
        let mut writer = BufWriter::new(file);
        let start = Instant::now();
        while result.write_bytes < max_bytes && start.elapsed() < timeout {
            let bytes_to_write =
                benchmark_calc_write_chunk(block_size, max_bytes, result.write_bytes);
            writer.write_all(&buffer[..bytes_to_write]).map_err(|_| {
                DiskBenchmarkError::Runtime("Failed to write data to the test file".to_string())
            })?;
            result.write_bytes += bytes_to_write as u64;
        }
        writer.flush().map_err(|_| {
            DiskBenchmarkError::Runtime("Failed to write data to the test file".to_string())
        })?;
        drop(writer);
        result.write_ms = start.elapsed().as_secs_f64() * 1000.0;
    }

    {
        let file = std::fs::File::open(&file_path).map_err(|_| {
            DiskBenchmarkError::Runtime("Failed to open test file for reading".to_string())
        })?;
        let mut reader = BufReader::new(file);
        let start = Instant::now();
        while start.elapsed() < timeout {
            let mut got = 0usize;
            while got < block_size {
                match reader.read(&mut buffer[got..block_size]) {
                    Ok(0) => break,
                    Ok(n) => got += n,
                    Err(_) => break,
                }
            }
            result.read_bytes += got as u64;
            if got < block_size {
                break;
            }
        }
        result.read_ms = start.elapsed().as_secs_f64() * 1000.0;
    }

    std::fs::remove_file(&file_path).map_err(|e| {
        DiskBenchmarkError::Runtime(format!(
            "Failed to delete test file {}: {}",
            file_path.display(),
            e
        ))
    })?;

    Ok(result)
}

// ============================================================================
// Util::IsControlChar, Util::StrToNum, Util::SafeIntCast, and RegEx (`Util.h`/`Util.cpp`)
// ============================================================================

/// Port of `Util::IsControlChar(char c)`.
pub const fn util_is_control_char(c: u8) -> bool {
    c < 32 || c == 127
}

fn c_terminated(s: &[u8]) -> Vec<c_char> {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    let mut out = Vec::with_capacity(end + 1);
    out.extend(s[..end].iter().map(|&b| b as c_char));
    out.push(0);
    out
}

/// Port of `Util::StrToNum<T>` for floating-point types.
pub fn util_str_to_f64_checked(s: &[u8], min_val: f64, max_val: f64) -> Option<f64> {
    let cbuf = c_terminated(s);
    let start = cbuf.as_ptr();
    let mut end: *mut c_char = std::ptr::null_mut();
    let val = unsafe { strtod(start, &mut end) };
    if end.cast_const() == start || val < min_val || val > max_val || val.is_nan() {
        None
    } else {
        Some(val)
    }
}

/// Port of `Util::StrToNum<T>` for signed integral types.
pub fn util_str_to_i64_checked(s: &[u8], min_val: i64, max_val: i64) -> Option<i64> {
    let cbuf = c_terminated(s);
    let start = cbuf.as_ptr();
    let mut end: *mut c_char = std::ptr::null_mut();
    let val = unsafe { strtoll(start, &mut end, 10) } as i64;
    if end.cast_const() == start || val < min_val || val > max_val {
        None
    } else {
        Some(val)
    }
}

/// Port of `Util::StrToNum<T>` for unsigned integral types.
pub fn util_str_to_u64_checked(s: &[u8], max_val: u64) -> Option<u64> {
    let cbuf = c_terminated(s);
    let start = cbuf.as_ptr();
    let mut end: *mut c_char = std::ptr::null_mut();
    let val = unsafe { strtoull(start, &mut end, 10) } as u64;
    if end.cast_const() == start || val > max_val {
        None
    } else {
        Some(val)
    }
}

pub trait StrToNumTarget: Sized {
    fn parse_str_to_num(s: &[u8]) -> Option<Self>;
}

impl StrToNumTarget for f32 {
    fn parse_str_to_num(s: &[u8]) -> Option<Self> {
        util_str_to_f64_checked(s, f32::MIN as f64, f32::MAX as f64).map(|v| v as f32)
    }
}

impl StrToNumTarget for f64 {
    fn parse_str_to_num(s: &[u8]) -> Option<Self> {
        util_str_to_f64_checked(s, f64::MIN, f64::MAX)
    }
}

macro_rules! impl_str_to_signed {
    ($($t:ty),*) => {
        $(impl StrToNumTarget for $t {
            fn parse_str_to_num(s: &[u8]) -> Option<Self> {
                util_str_to_i64_checked(s, <$t>::MIN as i64, <$t>::MAX as i64).map(|v| v as $t)
            }
        })*
    };
}

macro_rules! impl_str_to_unsigned {
    ($($t:ty),*) => {
        $(impl StrToNumTarget for $t {
            fn parse_str_to_num(s: &[u8]) -> Option<Self> {
                util_str_to_u64_checked(s, <$t>::MAX as u64).map(|v| v as $t)
            }
        })*
    };
}

impl_str_to_signed!(i8, i16, i32, i64);
impl_str_to_unsigned!(u8, u16, u32, u64);

pub fn util_str_to_num<T: StrToNumTarget>(s: &str) -> Option<T> {
    T::parse_str_to_num(s.as_bytes())
}

/// Port of `Util::SafeIntCast<From, To>` for signed -> signed.
pub const fn util_safe_int_cast_i64_to_i64(num: i64, min_to: i64, max_to: i64) -> i64 {
    if num < min_to || num > max_to {
        0
    } else {
        num
    }
}

/// Port of `Util::SafeIntCast<From, To>` for unsigned -> signed.
pub const fn util_safe_int_cast_u64_to_i64(num: u64, max_to: u64) -> i64 {
    if num > max_to {
        0
    } else {
        num as i64
    }
}

/// Port of `Util::SafeIntCast<From, To>` for signed -> unsigned.
pub const fn util_safe_int_cast_i64_to_u64(num: i64, max_to: u64) -> u64 {
    if num < 0 || (num as u64) > max_to {
        0
    } else {
        num as u64
    }
}

/// Port of `Util::SafeIntCast<From, To>` for unsigned -> unsigned.
pub const fn util_safe_int_cast_u64_to_u64(num: u64, max_to: u64) -> u64 {
    if num > max_to {
        0
    } else {
        num
    }
}

/// State & match-buffer manager for `RegEx` (`daemon/util/Util.cpp`).
pub struct PosixRegEx {
    valid: bool,
    match_buf_size: usize,
    matches: Vec<(i32, i32)>,
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    raw_ctx: Option<Box<[u64; 16]>>,
}

#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawRegMatch {
    #[cfg(target_os = "macos")]
    rm_so: i64,
    #[cfg(target_os = "macos")]
    rm_eo: i64,
    #[cfg(not(target_os = "macos"))]
    rm_so: c_int,
    #[cfg(not(target_os = "macos"))]
    rm_eo: c_int,
}

#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
extern "C" {
    fn regcomp(preg: *mut u64, pattern: *const c_char, cflags: c_int) -> c_int;
    fn regexec(
        preg: *const u64,
        string: *const c_char,
        nmatch: usize,
        pmatch: *mut RawRegMatch,
        eflags: c_int,
    ) -> c_int;
    fn regfree(preg: *mut u64);
}

#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
impl Drop for PosixRegEx {
    fn drop(&mut self) {
        if let Some(ref mut ctx) = self.raw_ctx {
            unsafe { regfree(ctx.as_mut_ptr()) };
        }
    }
}

impl PosixRegEx {
    pub fn new_with(valid: bool, match_buf_size: i32) -> Self {
        let cap = if match_buf_size > 0 {
            match_buf_size as usize
        } else {
            0
        };
        Self {
            valid,
            match_buf_size: cap,
            matches: vec![(-1, -1); cap],
            #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
            raw_ctx: None,
        }
    }

    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    pub fn compile(pattern: &CStr, match_buf_size: i32) -> Self {
        const REG_EXTENDED: c_int = 1;
        const REG_ICASE: c_int = 2;
        #[cfg(target_os = "macos")]
        const REG_NOSUB: c_int = 4;
        #[cfg(not(target_os = "macos"))]
        const REG_NOSUB: c_int = 8;

        let mut ctx = Box::new([0u64; 16]);
        let flags = REG_EXTENDED | REG_ICASE | if match_buf_size > 0 { 0 } else { REG_NOSUB };
        let rc = unsafe { regcomp(ctx.as_mut_ptr(), pattern.as_ptr(), flags) };
        let valid = rc == 0;
        let mut rx = Self::new_with(valid, match_buf_size);
        if valid {
            rx.raw_ctx = Some(ctx);
        } else {
            unsafe { regfree(ctx.as_mut_ptr()) };
        }
        rx
    }

    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    pub fn match_cstr(&mut self, text: &CStr) -> bool {
        let Some(ctx_ptr) = self.raw_ctx.as_ref().map(|b| b.as_ptr()) else {
            return false;
        };
        let nmatch = self.match_buf_size;
        self.exec_with(|matches| {
            let mut raw = vec![RawRegMatch::default(); nmatch];
            let pmatch = if nmatch > 0 {
                raw.as_mut_ptr()
            } else {
                std::ptr::null_mut()
            };
            let ok = unsafe { regexec(ctx_ptr, text.as_ptr(), nmatch, pmatch, 0) == 0 };
            if ok {
                for (i, slot) in matches.iter_mut().enumerate() {
                    *slot = (raw[i].rm_so as i32, raw[i].rm_eo as i32);
                }
            }
            ok
        })
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn exec_with(&mut self, exec_fn: impl FnOnce(&mut [(i32, i32)]) -> bool) -> bool {
        if !self.valid {
            return false;
        }
        for slot in &mut self.matches {
            *slot = (-1, -1);
        }
        exec_fn(&mut self.matches)
    }

    pub fn match_count(&self) -> i32 {
        let mut count = 0usize;
        while count < self.match_buf_size && self.matches[count].0 > -1 {
            count += 1;
        }
        count as i32
    }

    pub fn match_start(&self, index: i32) -> i32 {
        if index < 0 || (index as usize) >= self.matches.len() {
            0
        } else {
            self.matches[index as usize].0
        }
    }

    pub fn match_len(&self, index: i32) -> i32 {
        if index < 0 || (index as usize) >= self.matches.len() {
            0
        } else {
            let (so, eo) = self.matches[index as usize];
            regex_calc_match_len(so, eo)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_size(0), b"0 MB");
        assert_eq!(format_size(512), b"512 B");
        assert_eq!(format_size(1536), b"1.50 KB");
        assert_eq!(format_size(5 * 1024 * 1024 * 1024), b"5.00 GB");
        assert_eq!(format_speed(500), b"0 KB/s");
        assert_eq!(format_speed(85_496_208), b"81.5 MB/s");
    }

    #[test]
    fn hash() {
        // the init value and every byte count in the tail change the hash
        // (rust/tests/util_differential.py compares it with the C++)
        let h: Vec<u32> = (0..=24).map(|n| hash_bj96(&b"Four score and seven years ago"[..n], 0)).collect();
        assert!(h.windows(2).all(|w| w[0] != w[1]));
        assert_ne!(hash_bj96(b"x", 0), hash_bj96(b"x", 1));
    }

    #[test]
    fn reduce() {
        unsafe {
            for (raw, from, to, expected) in [
                (&b"TTTF|FT\0"[..], c"TT", c"T", &b"TF|FT\0\0\0"[..]),
                (&b"abc\0"[..], c"ab", c"xy", &b"xy\0\0"[..]),
            ] {
                let mut b = raw.to_vec();
                reduce_str(b.as_mut_ptr().cast(), from.as_ptr(), to.as_ptr());
                assert_eq!(b, expected);
            }
            let mut b = *b"ababX\0";
            reduce_str(b.as_mut_ptr().cast(), c"ab".as_ptr(), b.as_ptr().add(4).cast());
            assert_eq!(&b, b"XbX\0\0\0");
            // Initially equal operands are not a no-op: the first copy
            // truncates the aliased replacement, so the next deletes 'a'.
            let mut b = *b"aa\0";
            reduce_str(b.as_mut_ptr().cast(), c"a".as_ptr(), b.as_ptr().add(1).cast());
            assert_eq!(&b, b"\0\0\0");
        }
    }

    #[test]
    fn command_line_and_trims() {
        assert_eq!(split_command_line(b"a 'b c' 'd''e' f"), [b"a" as &[u8], b"b c", b"d'e", b"f"]);
        assert_eq!(trim_line(b" \tx y\r\n"), (2, 5));
        assert!(ends_with(b"file.NZB", b".nzb", false));
        assert!(!ends_with(b"file.NZB", b".nzb", true));
        assert_eq!(format_buffer(b"\x00\xff"), b"00 ff ");
        assert_eq!(parse_rfc822_date_time(c"Wed, 26 Jun 2013 01:02:54 -0600"), 1372230174);
        assert_eq!(parse_rfc822_date_time(c"26 Jun 2013 01:02 GMT"), 1372208520);
        assert_eq!(parse_rfc822_date_time(c"junk"), 0);
    }

    #[test]
    fn file_ext() {
        fn ascii(b: u8) -> std::ffi::c_int {
            b.to_ascii_lowercase() as std::ffi::c_int
        }
        let l = Lower::Fold(&ascii);
        let m = |f: &[u8], list: &[u8]| match_file_ext(f, list, b",;", &l, &l);
        assert!(m(b"a.NZB", b".zip, .nzb"));
        assert!(m(b"x.r01", b"*.r??"));
        assert!(!m(b"a.nzb", b".zip"));
        assert_eq!(tokens(b" a ,, b\t;", b",;").collect::<Vec<_>>(), [b"a" as &[u8], b"b"]);
    }

    #[test]
    fn internal_timegm_normalizes_months() {
        assert_eq!(
            util_internal_timegm(124, -1, 15, 0, 0, 0),
            util_internal_timegm(123, 11, 15, 0, 0, 0)
        );
        assert_eq!(
            util_internal_timegm(123, 12, 15, 0, 0, 0),
            util_internal_timegm(124, 0, 15, 0, 0, 0)
        );
    }

    #[test]
    fn data_analytics_port_from_cpp() {
        assert_eq!(data_analytics_compute(&mut []), None);
        let mut s1 = [5.0, 10.0, 15.0];
        assert_eq!(
            data_analytics_compute(&mut s1),
            Some((5.0, 15.0, 10.0, 10.0, 7.5, 12.5, 14.0))
        );
        let mut s2 = [5.0, 10.0, 15.0, 2.0, 8.0];
        assert_eq!(
            data_analytics_compute(&mut s2),
            Some((2.0, 15.0, 8.0, 8.0, 5.0, 10.0, 13.0))
        );
    }

    struct BenchTempDir {
        path: std::path::PathBuf,
    }

    impl BenchTempDir {
        fn new(prefix: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("{prefix}{nanos}_{}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn str(&self) -> String {
            self.path.to_string_lossy().into_owned()
        }

        fn file_count(&self) -> usize {
            std::fs::read_dir(&self.path)
                .map(|it| it.count())
                .unwrap_or(0)
        }
    }

    impl Drop for BenchTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn disk_benchmark_suite_port_from_cpp() {
        use std::time::Duration;
        const MIB: u64 = 1024 * 1024;
        const GIB: u64 = 1024 * MIB;
        let plenty = Ok(u64::MAX / 2);

        // 1. BenchmarkRejectsTooBigBufferTest
        let too_big = 1024usize * 1024 * 1024;
        assert!(matches!(
            disk_benchmark_run("./", too_big, 1024, Duration::from_secs(1), plenty.clone()),
            Err(DiskBenchmarkError::InvalidArgument(_))
        ));

        // 2. BenchmarkFailsOnInvalidPathTest
        assert!(matches!(
            disk_benchmark_run(
                "InvalidPath_nonexistent_dir_xyz",
                1024,
                1024,
                Duration::from_secs(1),
                plenty.clone()
            ),
            Err(DiskBenchmarkError::Runtime(_))
        ));

        // 3. BenchmarkReportsMissingDirectoryTest
        let dir = BenchTempDir::new("nzbget_bench_missing_");
        let missing = format!("{}/missing", dir.str());
        match disk_benchmark_run(&missing, 1024, 1024, Duration::from_secs(1), plenty.clone()) {
            Err(DiskBenchmarkError::Runtime(msg)) => {
                assert!(msg.contains("Directory does not exist"));
            }
            other => panic!("expected Runtime error, got {other:?}"),
        }
        assert!(!std::path::Path::new(&missing).exists());

        // 4. BenchmarkMeasuresWriteAndReadTest
        let dir = BenchTempDir::new("nzbget_bench_rw_");
        let res = disk_benchmark_run(
            &dir.str(),
            64 * 1024,
            4 * MIB,
            Duration::from_secs(5),
            plenty.clone(),
        )
        .unwrap();
        assert_eq!(res.write_bytes, 4 * MIB);
        assert_eq!(res.read_bytes, res.write_bytes);
        assert!(res.write_ms > 0.0);
        assert!(res.read_ms > 0.0);
        assert_eq!(dir.file_count(), 0);

        // 5. BenchmarkUtf8DirectoryTest
        let dir = BenchTempDir::new("nzbget_l\u{00e4}_\u{4f60}\u{597d}_");
        let res =
            disk_benchmark_run(&dir.str(), 0, MIB, Duration::from_secs(5), plenty.clone()).unwrap();
        assert!(res.write_bytes >= MIB);
        assert_eq!(res.read_bytes, res.write_bytes);
        assert_eq!(dir.file_count(), 0);

        // 6. BenchmarkStopsOnTimeoutTest
        let dir = BenchTempDir::new("nzbget_bench_timeout_");
        let res = disk_benchmark_run(
            &dir.str(),
            4096,
            1024 * GIB,
            Duration::from_millis(250),
            plenty.clone(),
        )
        .unwrap();
        assert!(res.write_bytes > 0);
        assert!(res.write_bytes < 1024 * GIB);
        assert!(res.read_bytes > 0);
        assert!(res.read_bytes <= res.write_bytes);
        assert_eq!(dir.file_count(), 0);

        // 7. BenchmarkZeroBlockSizeUsesDefaultTest
        let dir = BenchTempDir::new("nzbget_bench_zero_");
        let res = disk_benchmark_run(
            &dir.str(),
            0,
            4 * MIB,
            Duration::from_secs(5),
            plenty.clone(),
        )
        .unwrap();
        assert_eq!(res.write_bytes, 4 * MIB);
        assert_eq!(res.read_bytes, res.write_bytes);

        // 8. BenchmarkOddBlockSizeTest
        let dir = BenchTempDir::new("nzbget_bench_odd_");
        let res = disk_benchmark_run(
            &dir.str(),
            1000,
            10000,
            Duration::from_secs(5),
            plenty,
        )
        .unwrap();
        assert_eq!(res.write_bytes, 10000);
        assert_eq!(res.read_bytes, 10000);

        // 9. BenchmarkLimitByFreeSpaceTest
        let reserve = BENCHMARK_FREE_SPACE_RESERVE;
        assert_eq!(benchmark_limit_by_free_space(10 * GIB, 0, reserve), 0);
        assert_eq!(benchmark_limit_by_free_space(10 * GIB, reserve, reserve), 0);
        assert_eq!(
            benchmark_limit_by_free_space(10 * GIB, reserve + GIB, reserve),
            GIB
        );
        assert_eq!(
            benchmark_limit_by_free_space(MIB, reserve + GIB, reserve),
            MIB
        );

        // 10. Non-UTF-8 directory bytes do not cause UB and report missing directory cleanly
        match disk_benchmark_run_bytes(
            b"NonUtf8MissingPath_\xff\xfe",
            0,
            MIB,
            Duration::from_secs(5),
            Ok(100 * GIB),
        ) {
            Err(DiskBenchmarkError::Runtime(msg)) => {
                assert!(msg.contains("Directory does not exist"));
            }
            other => panic!("expected Runtime error for non-UTF-8 missing path, got {other:?}"),
        }
    }

    #[test]
    fn util_full_suite_port_from_cpp() {
        fn ascii_lower(b: u8) -> std::ffi::c_int {
            b.to_ascii_lowercase() as std::ffi::c_int
        }
        fn ascii_alpha(b: u8) -> bool {
            b.is_ascii_alphabetic()
        }

        // 1. XmlStripTagsTest
        let mut xml = b"<div><img style=\"margin-left:10px;margin-bottom:10px;float:right;\" src=\"https://xxx/cover.jpg\"/><ul><li>ID: 12345678</li><li>Name: <a href=\"https://xxx/12344\">Show name</a></li><li>Size: 3.00 GB </li><li>Attributes: Category - <a href=\"https://xxx/2040\">Movies > HD</a></li></li></ul></div>".to_vec();
        let expected_strip = b"                                                                                                        ID: 12345678         Name:                             Show name             Size: 3.00 GB          Attributes: Category -                            Movies > HD                         ";
        crate::text::xml_strip_tags(&mut xml);
        assert_eq!(xml.as_slice(), expected_strip);

        // 2. XmlDecodeTest
        let mut xml = b"Poster: Bob &lt;bob@home&gt; bad&mdash;and there&#039;s one thing".to_vec();
        let len = crate::text::xml_decode(&mut xml, &ascii_lower);
        assert_eq!(
            &xml[..len],
            b"Poster: Bob <bob@home> bad&mdash;and there's one thing"
        );

        // 3. XmlRemoveEntitiesTest & XmlRemoveEntitiesAtEndTest
        let mut xml = b"Poster: Bob &lt;bob@home&gt; bad&mdash;and there&#039;s one thing".to_vec();
        let len = crate::text::xml_remove_entities(&mut xml, &ascii_alpha);
        assert_eq!(
            &xml[..len],
            b"Poster: Bob  bob@home  bad and there s one thing"
        );
        let mut xml_end = b"R&D AT&T".to_vec();
        let len = crate::text::xml_remove_entities(&mut xml_end, &ascii_alpha);
        assert_eq!(&xml_end[..len], b"R&D AT&T");

        // 4. HttpUnquoteEscapesTest
        for (raw, expected) in [
            (&b"\"a\\\"b.nzb\""[..], &b"a\"b.nzb"[..]),
            (&b"\"my\\ file.nzb\""[..], &b"my file.nzb"[..]),
            (&b"\"a\\\\b\""[..], &b"a\\b"[..]),
        ] {
            let mut buf = raw.to_vec();
            let len = crate::text::http_unquote(&mut buf);
            assert_eq!(&buf[..len], expected);
        }

        // 5. EncodeByte80Test & JsonEncodeNonBmpTest
        let mut json80 = Vec::new();
        crate::escape::json_encode(b"a\x80b", &mut json80);
        let mut xml80 = Vec::new();
        crate::escape::xml_encode(b"a\x80b", &mut xml80);
        assert!(!json80.contains(&0x80));
        assert!(!xml80.contains(&0x80));

        let mut non_bmp = Vec::new();
        crate::escape::json_encode(b"x\xF0\x9F\x98\x80y", &mut non_bmp);
        assert_eq!(non_bmp, b"x\\ud83d\\ude00y");
        let mut decoded = non_bmp;
        let len = crate::decode::json_decode(&mut decoded);
        assert_eq!(&decoded[..len], b"x\xF0\x9F\x98\x80y");

        // 6. XmlDecodeCdataTest & URLEncodeTest
        let mut cdata = b"a<![CDATA[x<&amp;>y]]>b&amp;c".to_vec();
        let len = crate::text::xml_decode(&mut cdata, &ascii_lower);
        assert_eq!(&cdata[..len], b"ax<&amp;>yb&c");
        let mut open = b"a<![CDATA[x".to_vec();
        let len = crate::text::xml_decode(&mut open, &ascii_lower);
        assert_eq!(&open[..len], b"ax");
        let mut url_out = Vec::new();
        crate::text::url_encode(
            b"http://www.example.com/nzb_get/12344/Debian V7 6 64 bit OS.nzb",
            &mut url_out,
        );
        assert_eq!(
            url_out,
            b"http://www.example.com/nzb_get/12344/Debian%20V7%206%2064%20bit%20OS.nzb"
        );

        // 7. ParseRfc822DateTimeTest
        let t = 1372208574i64;
        assert_eq!(parse_rfc822_date_time(c"Wed, 26 Jun 2013 01:02:54 +0000"), t);
        assert_eq!(parse_rfc822_date_time(c"Wed, 26 Jun 2013 01:02:54 GMT"), t);
        assert_eq!(parse_rfc822_date_time(c"26 Jun 2013 01:02:54 +0000"), t);
        assert_eq!(parse_rfc822_date_time(c"Wed, 26 Jun 2013 03:02:54 +0200"), t);
        assert_eq!(parse_rfc822_date_time(c"Tue, 25 Jun 2013 19:02:54 -0600"), t);
        assert_eq!(parse_rfc822_date_time(c"Wed, 26 Jun 2013 00:32:54 -0030"), t);
        assert_eq!(parse_rfc822_date_time(c"Tue, 25 Jun 2013 21:02:54 EDT"), t);
        assert_eq!(parse_rfc822_date_time(c"26 Jun 2013 01:02 +0000"), t - 54);
        assert_eq!(parse_rfc822_date_time(c"not a date"), 0);

        // 8. ParseContentDispositionFilenameTest
        let lower = Lower::Fold(&ascii_lower);
        let cd = |hdr: &[u8]| {
            crate::webutil::content_disposition_filename(hdr, &lower).unwrap_or_default()
        };
        assert_eq!(cd(b"Content-Disposition: attachment; filename=\"fname.ext\""), b"fname.ext");
        assert_eq!(cd(b"Content-Disposition: attachement;filename=fname.ext"), b"fname.ext");
        assert_eq!(cd(b"Content-Disposition: attachement;filename=fname.ext;"), b"fname.ext");
        assert_eq!(
            cd(b"Content-Disposition: attachment; filename=1313961.nzb; filename*=UTF-8''1313961.nzb"),
            b"1313961.nzb"
        );
        assert_eq!(
            cd(b"Content-Disposition: attachment; filename=fallback.nzb; filename*=UTF-8''na%C3%AFve%20file.nzb"),
            b"na\xC3\xAFve file.nzb"
        );
        assert_eq!(
            cd(b"Content-Disposition: attachment; filename*=UTF-8'en'fname.ext"),
            b"fname.ext"
        );
        assert_eq!(
            cd(b"Content-Disposition: attachment; filename*=ISO-8859-1''na%EFve.nzb"),
            b"na\xC3\xAFve.nzb"
        );
        assert_eq!(
            cd(b"Content-Disposition: attachment; filename=\"semi;colon.nzb\"; foo=bar"),
            b"semi;colon.nzb"
        );
        assert_eq!(
            cd(b"Content-Disposition: attachment; FileName=\"fname.ext\""),
            b"fname.ext"
        );
        assert_eq!(
            cd(b"Content-Disposition: attachment; filename=fname.ext\r\n"),
            b"fname.ext"
        );
        assert!(cd(b"Content-Disposition: inline").is_empty());
        assert!(cd(b"Content-Disposition: attachment; filename=").is_empty());
        assert_eq!(
            cd(b"Content-Disposition: attachment; filename*=UTF-8''fname%"),
            b"fname%"
        );

        // 9. WildMaskTest & WildMaskCaptureBacktrackingTest
        assert!(!crate::wildmask::wild_match(&lower, b"*.par2", b"Debian V7 6 64 bit OS.nzb", None));
        assert!(!crate::wildmask::wild_match(&lower, b"*.par2", b"Debian V7 6 64 bit OS.par2.nzb", None));
        assert!(crate::wildmask::wild_match(&lower, b"*.par2", b"Debian V7 6 64 bit OS.par2", None));
        assert!(crate::wildmask::wild_match(&lower, b"*.par2", b".par2", None));
        assert!(!crate::wildmask::wild_match(&lower, b"*.par2", b"par2", None));

        let mut text = vec![b'a'; 200];
        text.push(b'b');
        let mut caps = Vec::new();
        assert!(crate::wildmask::wild_match(&lower, b"*?ab", &text, Some(&mut caps)));
        assert_eq!(caps.len(), 200);
        assert_eq!(caps[0], (0, 0));
        assert_eq!(caps[199], (198, 2));

        let mut failed_caps = Vec::new();
        assert!(!crate::wildmask::wild_match(&lower, b"?x", b"ay", Some(&mut failed_caps)));
        assert_eq!(failed_caps, vec![(0, 1)]);

        // 10. RegExTest (PosixRegEx)
        #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
        {
            let mut rx_rar = PosixRegEx::compile(c".*\\.rar$", 100);
            let mut rx_rar_seq = PosixRegEx::compile(c".*\\.[r-z][0-9][0-9]$", 100);
            let mut rx_7z = PosixRegEx::compile(c".*\\.7z$|.*\\.7z\\.[0-9]+$", 100);
            let mut rx_num = PosixRegEx::compile(c".*\\.[0-9]+$", 100);
            assert!(rx_rar.match_cstr(c"filename.rar"));
            assert!(rx_rar.match_cstr(c"filename.part001.rar"));
            assert!(!rx_rar.match_cstr(c"filename.rar.txt"));
            assert!(!rx_rar_seq.match_cstr(c"filename.rar"));
            assert!(rx_rar_seq.match_cstr(c"filename.r01"));
            assert!(rx_rar_seq.match_cstr(c"filename.r99"));
            assert!(!rx_rar_seq.match_cstr(c"filename.r001"));
            assert!(rx_rar_seq.match_cstr(c"filename.s01"));
            assert!(rx_rar_seq.match_cstr(c"filename.t99"));
            assert!(rx_7z.match_cstr(c"filename.7z"));
            assert!(!rx_7z.match_cstr(c"filename.7z.rar"));
            assert!(rx_7z.match_cstr(c"filename.7z.1"));
            assert!(rx_7z.match_cstr(c"filename.7z.001"));
            assert!(rx_7z.match_cstr(c"filename.7z.123"));
            assert!(rx_7z.match_cstr(c"filename.7z.999"));
            assert!(rx_num.match_cstr(c"filename.7z.1"));
            assert!(rx_num.match_cstr(c"filename.7z.9"));
            assert!(rx_num.match_cstr(c"filename.7z.001"));
            assert!(rx_num.match_cstr(c"filename.7z.123"));
            assert!(rx_num.match_cstr(c"filename.7z.999"));

            let mut se = PosixRegEx::compile(c".*S([0-9]+)E([0-9]+).*", 100);
            assert!(se.is_valid());
            assert!(se.match_cstr(c"My.Show.Name.S01E02.ABC.720"));
            assert_eq!(se.match_count(), 3);
            assert_eq!(se.match_start(1), 14);
            assert_eq!(se.match_len(1), 2);
        }
        let mut rx = PosixRegEx::new_with(true, 100);
        assert!(rx.is_valid());
        assert!(rx.exec_with(|matches| {
            matches[0] = (0, 27);
            matches[1] = (14, 16);
            matches[2] = (17, 19);
            true
        }));
        assert_eq!(rx.match_count(), 3);
        assert_eq!(rx.match_start(1), 14);
        assert_eq!(rx.match_len(1), 2);
        let mut invalid_rx = PosixRegEx::new_with(false, 10);
        assert!(!invalid_rx.is_valid());
        assert!(!invalid_rx.exec_with(|_| true));

        // 11. StrToNumTest
        assert!((util_str_to_num::<f32>("3.14").unwrap() - 3.14f32).abs() <= f32::EPSILON);
        assert!((util_str_to_num::<f32>("3.14 abc").unwrap() - 3.14f32).abs() <= f32::EPSILON);
        assert_eq!(util_str_to_num::<f64>("3."), Some(3.0));
        assert_eq!(util_str_to_num::<i32>("3."), Some(3));
        assert_eq!(util_str_to_num::<i32>("3"), Some(3));
        assert_eq!(util_str_to_num::<i32>("-3"), Some(-3));
        assert_eq!(util_str_to_num::<i32>("+3"), Some(3));
        assert_eq!(util_str_to_num::<i32>("   3"), Some(3));
        assert_eq!(util_str_to_num::<i32>("3   "), Some(3));
        assert_eq!(util_str_to_num::<i32>("3 not a number"), Some(3));
        assert_eq!(util_str_to_num::<u8>("256"), None);
        assert_eq!(util_str_to_num::<i32>("not a number"), None);
        assert_eq!(util_str_to_num::<f32>(""), None);
        assert_eq!(util_str_to_num::<f32>("  "), None);

        // 12. StrCaseCmpTest, SplitInt64Test, FormatSpeedTest
        assert!(bytes_eq_ignore_case(b"OPTIONS", b"options"));
        assert!(!bytes_eq_ignore_case(b"OPTIONS", b"opt1ons"));
        assert!(bytes_eq_ignore_case(b"0PTIONS", b"0ptions"));
        assert!(bytes_eq_ignore_case(b"", b""));
        assert!(!bytes_eq_ignore_case(b"3.14", b"3.12"));

        for val in [i64::MAX, i64::MIN, 0i64] {
            let (hi, lo) = util_split_int64(val);
            assert_eq!(util_join_int64(hi, lo), val);
        }

        const K: i64 = 1024;
        assert_eq!(format_speed(K * K * K), b"1.00 GB/s");
        assert_eq!(format_speed(10 * K * K * K), b"10.0 GB/s");
        assert_eq!(format_speed(100 * K * K * K), b"100 GB/s");
        assert_eq!(format_speed(K * K), b"1.00 MB/s");
        assert_eq!(format_speed(10 * K * K), b"10.0 MB/s");
        assert_eq!(format_speed(100 * K * K), b"100 MB/s");
        assert_eq!(format_speed(K), b"1 KB/s");
        assert_eq!(format_speed(10 * K), b"10 KB/s");
        assert_eq!(format_speed(100 * K), b"100 KB/s");

        // 13. SafeIntCastTest
        let i32_min = i32::MIN as i64;
        let i32_max = i32::MAX as i64;
        assert_eq!(util_safe_int_cast_i64_to_i64(i64::MAX, i32_min, i32_max), 0);
        assert_eq!(util_safe_int_cast_i64_to_i64(i64::MIN, i32_min, i32_max), 0);
        assert_eq!(util_safe_int_cast_i64_to_i64(i32_max, i32_min, i32_max), i32_max);
        assert_eq!(util_safe_int_cast_i64_to_i64(i32_min, i32_min, i32_max), i32_min);
        assert_eq!(util_safe_int_cast_u64_to_i64(u64::MAX, i32::MAX as u64), 0);
        assert_eq!(util_safe_int_cast_u64_to_i64(0, i32::MAX as u64), 0);
        assert_eq!(
            util_safe_int_cast_u64_to_i64(i32::MAX as u64, i32::MAX as u64),
            i32_max
        );
        assert_eq!(
            util_safe_int_cast_i64_to_u64(i32_max, u32::MAX as u64),
            i32::MAX as u64
        );
        assert_eq!(util_safe_int_cast_i64_to_u64(i32_min, u32::MAX as u64), 0);

        // 14. TrimTest & SanitizeLineTest & IsControlChar
        assert!(util_is_control_char(0));
        assert!(util_is_control_char(b'\r'));
        assert!(util_is_control_char(b'\n'));
        assert!(util_is_control_char(b'\t'));
        assert!(util_is_control_char(127));
        assert!(!util_is_control_char(b' '));
        assert!(!util_is_control_char(b'A'));
        assert!(!util_is_control_char(b'z'));
        assert!(!util_is_control_char(0xC3));
        assert!(!util_is_control_char(0xA9));

        for (raw, expected) in [
            ("\r\n\t  hello  \r\n", "hello"),
            ("TV\r\nHD", "TV  HD"),
            ("Movies\t4K", "Movies 4K"),
            ("\r\n  S\u{00e9}ries / Vid\u{00e9}o  \n", "S\u{00e9}ries / Vid\u{00e9}o"),
            ("M\u{00e4}dchen\r\n4K", "M\u{00e4}dchen  4K"),
        ] {
            let mut s = raw.as_bytes().to_vec();
            let (st, en) = sanitize_line(&mut s, |b| b.is_ascii_whitespace());
            assert_eq!(&s[st..en], expected.as_bytes());
        }

        // 15. JsonNextValue & JsonDecodeStopsAtEndTest & XmlDecodeStopsAtEndTest & NumericEscapesDecodeToUtf8Test
        assert_eq!(
            crate::decode::json_next_value(br#""hello\"\"\\ world""#),
            Some((0, 19))
        );
        assert_eq!(
            crate::decode::json_next_value(br#"  "hello world"  "#),
            Some((2, 13))
        );
        assert_eq!(crate::decode::json_next_value(b"12345,"), Some((0, 5)));
        assert_eq!(crate::decode::json_next_value(b"  true  }"), Some((2, 4)));
        assert_eq!(crate::decode::json_next_value(b"\tnull\n"), Some((1, 4)));
        assert_eq!(crate::decode::json_next_value(b""), None);
        assert_eq!(crate::decode::json_next_value(b"   \t\n "), None);
        assert_eq!(crate::decode::json_next_value(br#""dangling\""#), None);

        let mut short_escape = b"ab\\u4".to_vec();
        let len = crate::decode::json_decode(&mut short_escape);
        assert_eq!(&short_escape[..len], b"ab");

        let mut numeric_xml = b"ab&#65".to_vec();
        let len = crate::text::xml_decode(&mut numeric_xml, &ascii_lower);
        assert_eq!(&numeric_xml[..len], b"abA");
    }
}
