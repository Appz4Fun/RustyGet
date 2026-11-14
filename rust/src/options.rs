//! Options' value parsers (Options.cpp): ParseTime and ParseWeekDays (the
//! scheduler's task times), ValidateOptionName's classification of option
//! names, ConvertOldOption's renames, HasScript and ParseCategorySource, and
//! OptEntry::Restricted's protected option names.
//! Names are compared with the C library's strcasecmp/strncasecmp and numbers
//! read with atoi/strtol, in the current locale, as the C++ did.

use std::ffi::{c_char, c_int, c_long, CStr, CString};

fn eq(a: &CStr, b: &CStr) -> bool {
    a.to_bytes().eq_ignore_ascii_case(b.to_bytes())
}

fn eq_n(a: &CStr, b: &CStr, n: usize) -> bool {
    let ab = a.to_bytes();
    let bb = b.to_bytes();
    let alen = ab.len().min(n);
    let blen = bb.len().min(n);
    ab[..alen].eq_ignore_ascii_case(&bb[..blen]) && (alen == blen)
}

fn c_strtol(s: &[u8], base: i32) -> (c_long, bool) {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let start = i;
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let radix = if base == 0 {
        if i + 1 < s.len() && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') {
            i += 2;
            16u32
        } else if i < s.len() && s[i] == b'0' {
            8u32
        } else {
            10u32
        }
    } else {
        if base == 16 && i + 1 < s.len() && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') {
            i += 2;
        }
        base as u32
    };
    let digit_start = i;
    let mut acc: u64 = 0;
    let mut overflow = false;
    while i < s.len() {
        let Some(d) = (s[i] as char).to_digit(radix) else {
            break;
        };
        if let Some(next) = acc
            .checked_mul(radix as u64)
            .and_then(|v| v.checked_add(d as u64))
        {
            acc = next;
        } else {
            overflow = true;
        }
        i += 1;
    }
    if i == digit_start {
        if (base == 16 || base == 0) && digit_start >= start + 2 && s[digit_start - 2] == b'0' {
            return (0, digit_start - 1 == s.len());
        }
        return (0, s.is_empty());
    }
    let max_pos = c_long::MAX as u64;
    let max_neg = (c_long::MIN as i128).unsigned_abs() as u64;
    let val = if neg {
        if overflow || acc >= max_neg {
            c_long::MIN
        } else {
            -(acc as c_long)
        }
    } else if overflow || acc >= max_pos {
        c_long::MAX
    } else {
        acc as c_long
    };
    (val, i == s.len())
}

/// The C string from byte `at` of `s` (at most its length).
fn from(s: &CStr, at: usize) -> &CStr {
    let bytes = s.to_bytes_with_nul();
    CStr::from_bytes_with_nul(&bytes[at.min(bytes.len() - 1)..]).expect("one NUL at the end")
}

/// Options::ParseTime: `*` (a startup task: hours -1), `*:MM` (every hour:
/// hours -2) or `HH:MM`. Writes the outputs as the C++ did, also on failure.
pub fn parse_time(time: &CStr, hours: &mut i32, minutes: &mut i32) -> bool {
    parse_time_with(time, |h| *hours = h, |m| *minutes = m)
}

// Keep writes ordered and optional: C callers may alias the two outputs or
// supply uninitialized storage. Creating two &mut references would be UB.
pub(crate) fn parse_time_with(
    time: &CStr,
    mut hours: impl FnMut(i32),
    mut minutes: impl FnMut(i32),
) -> bool {
    let t = time.to_bytes();
    if t == b"*" {
        hours(-1);
        minutes(0);
        return true;
    }
    if t.iter().any(|c| !b"0123456789: *".contains(c)) {
        return false;
    }
    if t.iter().filter(|&&c| c == b':').count() != 1 {
        return false;
    }
    let colon = t.iter().position(|&c| c == b':').expect("one colon");
    if t[0] == b'*' {
        hours(-2);
    } else {
        let h = crate::feed::c_atoi_bytes(t);
        hours(h);
        if !(0..=23).contains(&h) {
            return false;
        }
    }
    if t.get(colon + 1) == Some(&b'*') {
        return false;
    }
    let m = crate::feed::c_atoi_bytes(from(time, colon + 1).to_bytes());
    minutes(m);
    (0..=59).contains(&m)
}

/// Options::ParseWeekDays: days 1 (Monday) to 7, lists and ranges (`1-5,7`)
/// as bits (bit n: day n + 1). Writes the bits as the C++ did, also on failure.
pub fn parse_week_days(week_days: &[u8], bits: &mut i32) -> bool {
    *bits = 0;
    let mut first_day = 0;
    let mut range = false;
    for &c in week_days {
        match c {
            b'1'..=b'7' => {
                let day = (c - b'0') as i32;
                if range {
                    if day <= first_day || first_day == 0 {
                        return false;
                    }
                    for i in first_day..=day {
                        *bits |= 1 << (i - 1);
                    }
                    first_day = 0;
                } else {
                    *bits |= 1 << (day - 1);
                    first_day = day;
                }
                range = false;
            }
            b',' => range = false,
            b'-' => range = true,
            b' ' => {}
            _ => return false,
        }
    }
    true
}

/// What Options::ValidateOptionName decided (and logged).
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(i32)]
pub enum NameCheck {
    /// not an option (or a read-only one)
    Invalid = 0,
    Valid = 1,
    /// obsolete, ignored: a warning
    Obsolete = 2,
    /// an obsolete log option (use WriteLog): a warning
    ObsoleteLog = 4,
}

/// The numbered sections and their option suffixes.
const SECTIONS: [(&CStr, usize, &[&CStr]); 4] = [
    (
        c"server",
        6,
        &[
            c".active",
            c".name",
            c".level",
            c".host",
            c".port",
            c".username",
            c".password",
            c".joingroup",
            c".encryption",
            c".connections",
            c".cipher",
            c".group",
            c".retention",
            c".optional",
            c".notes",
            c".ipversion",
            c".certverification",
        ],
    ),
    (
        c"task",
        4,
        &[
            c".time",
            c".weekdays",
            c".command",
            c".param",
            c".downloadrate",
            c".process",
        ],
    ),
    (
        c"category",
        8,
        &[c".name", c".destdir", c".unpack", c".aliases"],
    ),
    (
        c"feed",
        4,
        &[
            c".name",
            c".url",
            c".interval",
            c".filter",
            c".backlog",
            c".pausenzb",
            c".category",
            c".categorySource",
            c".priority",
            c".certverification",
        ],
    ),
];

const OBSOLETE: [&CStr; 22] = [
    c"RetryOnCrcError",
    c"AllowReProcess",
    c"LoadPars",
    c"ThreadLimit",
    c"PostLogKind",
    c"NZBLogKind",
    c"ProcessLogKind",
    c"AppendNzbDir",
    c"RenameBroken",
    c"MergeNzb",
    c"StrictParName",
    c"ReloadUrlQueue",
    c"ReloadPostQueue",
    c"ParCleanupQueue",
    c"DeleteCleanupDisk",
    c"HistoryCleanupDisk",
    c"SaveQueue",
    c"ReloadQueue",
    c"TerminateTimeout",
    c"AccurateRate",
    c"CreateBrokenLog",
    c"BrokenLog",
];

/// Options::ValidateOptionName; `predefined` tells whether the name is one of
/// the options (GetOption), asked only after the read-only check.
pub fn validate_option_name(name: &CStr, predefined: &mut dyn FnMut() -> bool) -> NameCheck {
    if [c"ConfigFile", c"AppBin", c"AppDir", c"Version"]
        .iter()
        .any(|n| eq(name, n))
    {
        return NameCheck::Invalid;
    }
    if predefined() {
        return NameCheck::Valid;
    }
    for (prefix, len, suffixes) in SECTIONS {
        if eq_n(name, prefix, len) {
            let digits = name.to_bytes()[len.min(name.to_bytes().len())..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
            let rest = from(name, len + digits);
            if suffixes.iter().any(|s| eq(rest, s)) {
                return NameCheck::Valid;
            }
        }
    }
    if name.to_bytes().contains(&b':') {
        return NameCheck::Valid;
    }
    if OBSOLETE.iter().any(|n| eq(name, n)) {
        return NameCheck::Obsolete;
    }
    if is_removed_extension_opt(name) {
        return NameCheck::Obsolete;
    }
    if [c"CreateLog", c"ResetLog"].iter().any(|n| eq(name, n)) {
        return NameCheck::ObsoleteLog;
    }
    NameCheck::Invalid
}

/// Options of the removed extension scripts feature, still accepted in old
/// configuration files and ignored with an "obsolete" warning.
fn is_removed_extension_opt(name: &CStr) -> bool {
    const GLOBAL: [&CStr; 11] = [
        c"ScriptDir",
        c"ScriptOrder",
        c"Extensions",
        c"ScanScript",
        c"QueueScript",
        c"FeedScript",
        c"PostProcess",
        c"NZBProcess",
        c"NZBAddedProcess",
        c"DefScript",
        c"PostScript",
    ];
    if GLOBAL.iter().any(|n| eq(name, n)) {
        return true;
    }
    for (prefix, len, suffixes) in [
        (
            c"category",
            8,
            &[c".extensions", c".defscript", c".postscript"][..],
        ),
        (c"feed", 4, &[c".extensions", c".feedscript"][..]),
    ] {
        if eq_n(name, prefix, len) {
            let digits = name.to_bytes()[len.min(name.to_bytes().len())..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
            let rest = from(name, len + digits);
            if suffixes.iter().any(|s| eq(rest, s)) {
                return true;
            }
        }
    }
    false
}

fn cstring(v: Vec<u8>) -> CString {
    CString::new(v).expect("no NUL: built from C strings")
}

/// Options::ConvertOldOption: the current name and value of an old option.
pub fn convert_old_option(option: &CStr, value: &CStr) -> (CString, CString) {
    let mut option = option.to_owned();
    let mut value = value.to_owned();
    for (old, new) in [
        (c"$MAINDIR", c"MainDir"),
        (c"ServerIP", c"ControlIP"),
        (c"ServerPort", c"ControlPort"),
        (c"ServerPassword", c"ControlPassword"),
        (c"PostPauseQueue", c"ScriptPauseQueue"),
    ] {
        if eq(&option, old) {
            option = new.to_owned();
        }
    }
    if eq(&option, c"ParCheck") && eq(&value, c"yes") {
        value = c"always".to_owned();
    }
    if eq(&option, c"ParCheck") && eq(&value, c"no") {
        value = c"auto".to_owned();
    }
    if eq(&option, c"ParScan") && eq(&value, c"auto") {
        value = c"extended".to_owned();
    }
    if eq(&option, c"WriteBufferSize") {
        option = c"WriteBuffer".to_owned();
        // strtol's long cut to an int, as the C++ stored it
        let val = c_strtol(value.to_bytes(), 10).0 as c_int;
        let val = if val == -1 { 1024 } else { val / 1024 };
        value = cstring(val.to_string().into_bytes());
    }
    for (old, new) in [
        (c"ConnectionTimeout", c"ArticleTimeout"),
        (c"Retries", c"ArticleRetries"),
        (c"RetryInterval", c"ArticleInterval"),
        (c"DumpCore", c"CrashDump"),
    ] {
        if eq(&option, old) {
            option = new.to_owned();
        }
    }
    if eq(&option, c"Decode") {
        option = c"RawArticle".to_owned();
        value = if eq(&value, c"no") { c"yes" } else { c"no" }.to_owned();
    }
    if eq(&option, c"LogBufferSize") {
        option = c"LogBuffer".to_owned();
    }
    (option, value)
}

/// Options::ParseCategorySource: 0 Auto, 1 NZBFile (also the default), 2 FeedFile.
pub fn parse_category_source(value: Option<&CStr>) -> i32 {
    let Some(value) = value else { return 1 };
    if eq_n(value, c"auto", 4) {
        0
    } else if eq_n(value, c"nzbfile", 8) {
        1
    } else if eq_n(value, c"feedfile", 9) {
        2
    } else {
        1
    }
}

/// `Options::CheckOptions`: converts `TimeCorrection` (hours if in `[-24, 24]`, else minutes) to seconds.
#[inline]
pub fn normalize_time_correction(raw_val: i32) -> i32 {
    let mins = if (-24..=24).contains(&raw_val) {
        raw_val * 60
    } else {
        raw_val
    };
    mins * 60
}

/// `Options::CheckOptions`: checks 32-bit memory limits for `ArticleCache` and `ParBuffer`.
/// Returns `(article_cache, par_buffer, err_kind)` where `err_kind`:
/// 0 = ok, 1 = article_cache > 1900, 2 = par_buffer > 1900, 3 = sum > 1900.
#[inline]
pub fn clamp_32bit_memory_limits(
    is_32bit: bool,
    article_cache: i32,
    par_buffer: i32,
) -> (i32, i32, i32) {
    let mut ac = article_cache;
    let mut pb = par_buffer;
    let mut err = 0;
    if ac < 0 {
        ac = 0;
    } else if is_32bit && ac > 1900 {
        ac = 1900;
        err = 1;
    } else if is_32bit && pb > 1900 {
        pb = 1900;
        err = 2;
    }
    if is_32bit && pb.saturating_add(ac) > 1900 {
        ac = 1500;
        pb = 400;
        err = 3;
    }
    (ac, pb, err)
}

/// OptEntry::Restricted's options whose value is protected, compared with strcasecmp.
const PROTECTED: [&CStr; 13] = [
    c"ControlIP",
    c"ControlPort",
    c"FormAuth",
    c"JsonP",
    c"SecureControl",
    c"SecurePort",
    c"SecureCert",
    c"SecureKey",
    c"CertStore",
    c"CertCheck",
    c"AuthorizedIP",
    c"DaemonUsername",
    c"UMask",
];

/// Options::OptEntry::Restricted: whether an option's value is protected (shown
/// as "***" to the restricted user): the options above, extension options (a
/// ':' in the name), and names holding "username", "password" or "apikey".
/// Those are searched in the first 1023 bytes (BString<1024>), lowercased with
/// the C library's tolower on each char, as the C++ did. signed_char describes
/// the C++ caller's plain char, which compiler flags can change independently
/// of Rust's c_char target default.
pub fn option_restricted(name: &CStr, _signed_char: bool) -> bool {
    let bytes = name.to_bytes();
    if PROTECTED.iter().any(|known| eq(name, known)) || bytes.contains(&b':') {
        return true;
    }
    let mut buf = [0u8; 1023];
    let lower = &mut buf[..bytes.len().min(1023)];
    for (l, &b) in lower.iter_mut().zip(bytes) {
        *l = b.to_ascii_lowercase();
    }
    [&b"username"[..], b"password", b"apikey"]
        .iter()
        .any(|needle| lower.windows(needle.len()).any(|w| w == *needle))
}

/// Options::ParseEnumValue: returns `(valid, enum_value, normalized_argn_index)`.
pub fn parse_enum_value(
    val: &CStr,
    def_val: &CStr,
    argn: &[&CStr],
    argv: &[i32],
) -> (bool, i32, Option<usize>) {
    let count = argn.len().min(argv.len());
    if count == 0 {
        return (false, 0, None);
    }
    let mut def_num = 0usize;
    for i in 0..count {
        if eq(val, argn[i]) {
            let mut norm_idx = None;
            for j in 0..count {
                if argv[j] == argv[i] {
                    if argn[j].to_bytes() != val.to_bytes() {
                        norm_idx = Some(j);
                    }
                    break;
                }
            }
            return (true, argv[i], norm_idx);
        }
        if eq(def_val, argn[i]) {
            def_num = i;
        }
    }
    (false, argv[def_num], Some(def_num))
}

/// Options::ParseIntValue: parses `val` with `strtol(val, &endptr, base)` (truncated to `i32`).
/// If `*endptr != '\0'`, returns `(false, strtol(def_val, null, base) as i32)`.
pub fn parse_int_value(val: &CStr, def_val: &CStr, base: i32) -> (bool, i32) {
    let (parsed, full) = c_strtol(val.to_bytes(), base);
    if !full {
        let (fallback, _) = c_strtol(def_val.to_bytes(), base);
        (false, fallback as i32)
    } else {
        (true, parsed as i32)
    }
}

/// Resolves the fallback `param` source (`0=param, 1=process, 2=downloadRate`) and
/// `(definition, completed)` flags for `Options::InitScheduler`.
pub fn options_resolve_scheduler_task_state(
    has_time: bool,
    has_weekdays: bool,
    has_command: bool,
    has_download_rate: bool,
    has_param_ptr: bool,
    param_empty: bool,
    process_empty: bool,
    download_rate_empty: bool,
) -> (i32, bool, bool) {
    let param_source = if param_empty && !process_empty {
        1
    } else if param_empty && !download_rate_empty {
        2
    } else {
        0
    };
    let has_effective_param = (param_source != 0) || has_param_ptr;
    let definition =
        has_time || has_weekdays || has_command || has_download_rate || has_effective_param;
    let completed = has_time && has_command;
    (param_source, definition, completed)
}

/// Splits an `option=value` string at the first `=` where the option name is non-empty
/// (`Options::SplitOptionString`).
pub fn options_split_option_string(option: &[u8]) -> Option<(&[u8], &[u8])> {
    option
        .iter()
        .position(|&b| b == b'=')
        .filter(|&eq| eq > 0)
        .map(|eq| (&option[..eq], &option[eq + 1..]))
}

/// Formats `"<dir><sep>nzbget.conf"` for `Options::InitOptFile` and `Options::CheckOptions`.
pub fn options_format_default_conf_path(dir: &[u8], sep: u8) -> Vec<u8> {
    const SUFFIX: &[u8] = b"nzbget.conf";
    let mut out = Vec::with_capacity(dir.len() + 1 + SUFFIX.len());
    out.extend_from_slice(dir);
    out.push(sep);
    out.extend_from_slice(SUFFIX);
    out
}

/// Returns the byte length of `dir` after stripping a single trailing `sep` for `Options::CheckDir`.
pub fn options_strip_trailing_sep(dir: &[u8], sep: u8) -> usize {
    if !dir.is_empty() && dir[dir.len() - 1] == sep {
        dir.len() - 1
    } else {
        dir.len()
    }
}

/// Formats `"<parent_dir><dir>"` (`join_mode == 1`) or `"<parent_dir><sep><dir>"` (`join_mode != 1`)
/// for `Options::CheckDir`.
pub fn options_format_joined_dir(
    parent_dir: &[u8],
    sep: u8,
    dir: &[u8],
    join_mode: i32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(parent_dir.len() + 1 + dir.len());
    out.extend_from_slice(parent_dir);
    if join_mode != 1 {
        out.push(sep);
    }
    out.extend_from_slice(dir);
    out
}

/// Converts minutes to seconds (`minutes * 60`) for `Options::InitOptions`.
pub fn options_minutes_to_seconds_i32(minutes: i32) -> i32 {
    minutes.wrapping_mul(60)
}

/// Formats `"<prefix><idx>.<field>"` for `Options::InitServers`, `Options::InitCategories`,
/// `Options::InitFeeds`, and `Options::InitScheduler`.
pub fn options_format_indexed_opt_name(prefix: &[u8], idx: i32, field: &[u8]) -> Vec<u8> {
    let idx_str = idx.to_string();
    let mut out = Vec::with_capacity(prefix.len() + idx_str.len() + 1 + field.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(idx_str.as_bytes());
    out.push(b'.');
    out.extend_from_slice(field);
    out
}

/// Returns `true` when `!no_disk_access` and `value` starts with `"~/"` for `Options::SetOption`.
pub fn options_should_expand_home_value(value: Option<&[u8]>, no_disk_access: bool) -> bool {
    !no_disk_access && value.is_some_and(|v| v.starts_with(b"~/"))
}

/// Resolves `m_configLine` in `Options::LocateOptionSrcPos`.
pub fn options_resolve_locate_src_line(found: bool, line_no: i32) -> i32 {
    if found {
        line_no
    } else {
        0
    }
}

/// Updates `m_configLine` in `Options::GetOption` when `line_no > 0`.
pub fn options_update_config_line_on_get(line_no: i32, cur_config_line: i32) -> i32 {
    if line_no > 0 {
        line_no
    } else {
        cur_config_line
    }
}

/// Returns true when a parsed scheduler task hour represents every hour (`hours == -2`).
pub fn options_is_every_hour_task(hours: i32) -> bool {
    hours == -2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_days() {
        let (mut h, mut m) = (99, 99);
        assert!(parse_time(c"*", &mut h, &mut m) && (h, m) == (-1, 0));
        assert!(parse_time(c"*:30", &mut h, &mut m) && (h, m) == (-2, 30));
        assert!(parse_time(c"23:59", &mut h, &mut m) && (h, m) == (23, 59));
        assert!(!parse_time(c"24:00", &mut h, &mut m));
        assert!(!parse_time(c"1:*", &mut h, &mut m));
        assert!(!parse_time(c"1:2:3", &mut h, &mut m));
        let mut b = 0;
        assert!(parse_week_days(b"1-5, 7", &mut b) && b == 0b1011111);
        assert!(!parse_week_days(b"5-1", &mut b));
    }

    #[test]
    fn names() {
        let mut no = || false;
        assert_eq!(
            validate_option_name(c"Server12.Host", &mut no),
            NameCheck::Valid
        );
        assert_eq!(
            validate_option_name(c"server.port", &mut no),
            NameCheck::Valid
        );
        assert_eq!(
            validate_option_name(c"Feed3.CategorySource", &mut no),
            NameCheck::Valid
        );
        assert_eq!(
            validate_option_name(c"AppDir", &mut || true),
            NameCheck::Invalid
        );
        assert_eq!(
            validate_option_name(c"Script.sh:Opt", &mut no),
            NameCheck::Valid
        );
        assert_eq!(
            validate_option_name(c"loadpars", &mut no),
            NameCheck::Obsolete
        );
        assert_eq!(validate_option_name(c"Bogus", &mut no), NameCheck::Invalid);
    }

    #[test]
    fn old_options() {
        // the removed extension options are not renamed any more
        let (o, v) = convert_old_option(c"Category2.DefScript", c"x");
        assert_eq!(
            (o.as_bytes(), v.as_bytes()),
            (b"Category2.DefScript".as_slice(), b"x".as_slice())
        );
        let (o, v) = convert_old_option(c"WriteBufferSize", c"-1");
        assert_eq!(
            (o.as_bytes(), v.as_bytes()),
            (b"WriteBuffer".as_slice(), b"1024".as_slice())
        );
        assert_eq!(convert_old_option(c"Decode", c"NO").1.as_bytes(), b"yes");
        assert_eq!(parse_category_source(Some(c"FeedFile")), 2);
    }

    #[test]
    fn restricted_options() {
        for name in [
            c"ControlPassword",
            c"Server1.Username",
            c"server2.PASSWORD",
            c"DupeSearchApiKey",
            c"JsonP",
            c"jsonp",
            c"UMASK",
            c"AuthorizedIP",
            c"Ext:Option",
            c":",
        ] {
            assert!(option_restricted(name, c_char::MIN < 0), "{name:?}");
        }
        for name in [
            c"DupeSearchUrl",
            c"DupeCheck",
            c"ControlIPs",
            c"JsonPx",
            c"Server1.Host",
            c"",
        ] {
            assert!(!option_restricted(name, c_char::MIN < 0), "{name:?}");
        }
        // only the first 1023 bytes are searched for the words; a ':' anywhere counts
        let mut long = vec![b'a'; 1020];
        long.extend_from_slice(b"password");
        assert!(!option_restricted(
            &CString::new(long.clone()).unwrap(),
            c_char::MIN < 0
        ));
        long.truncate(1015);
        long.extend_from_slice(b"password");
        assert!(option_restricted(
            &CString::new(long.clone()).unwrap(),
            c_char::MIN < 0
        ));
        long = vec![b'b'; 3023];
        assert!(!option_restricted(
            &CString::new(long.clone()).unwrap(),
            c_char::MIN < 0
        ));
        long.push(b':');
        assert!(option_restricted(
            &CString::new(long).unwrap(),
            c_char::MIN < 0
        ));
    }

    #[test]
    fn removed_extension_options_and_script_command() {
        // the extension scripts feature is gone: its options are accepted as
        // obsolete and the scheduler "Script" command is unknown
        for name in [
            c"ScriptDir",
            c"extensions",
            c"PostScript",
            c"DefScript",
            c"Category3.Extensions",
            c"Category1.PostScript",
            c"Feed2.FeedScript",
        ] {
            assert_eq!(
                validate_option_name(name, &mut || false),
                NameCheck::Obsolete
            );
        }
        assert_eq!(
            validate_option_name(c"Category3.Unpack", &mut || false),
            NameCheck::Valid
        );
        assert_eq!(options_lookup_scheduler_command(b"Script"), -1);
        assert_eq!(options_lookup_scheduler_command(b"process"), 6);
        assert_eq!(COMMAND_NAMES.len(), COMMAND_VALUES.len());
        assert!(!COMMAND_NAMES.iter().any(|n| eq(n, c"script")));
    }

    #[test]
    fn enum_and_int_values() {
        let names = [c"yes", c"no", c"true", c"false"];
        let vals = [1, 0, 1, 0];
        assert_eq!(
            parse_enum_value(c"TRUE", c"no", &names, &vals),
            (true, 1, Some(0))
        );
        assert_eq!(
            parse_enum_value(c"yes", c"no", &names, &vals),
            (true, 1, None)
        );
        assert_eq!(
            parse_enum_value(c"bogus", c"no", &names, &vals),
            (false, 0, Some(1))
        );

        assert_eq!(parse_int_value(c"42", c"10", 10), (true, 42));
        assert_eq!(parse_int_value(c"42abc", c"10", 10), (false, 10));
    }
}

/// `Options::CheckDir`: determines whether `dir` is relative to a non-empty
/// `parent_dir` and how they should be joined.
pub fn options_check_dir_join_mode(
    dir: &[u8],
    parent_dir: &[u8],
    path_sep: u8,
    alt_path_sep: u8,
) -> i32 {
    if parent_dir.is_empty() {
        return 0;
    }
    let d0 = dir.first().copied().unwrap_or(0);
    let d1 = dir.get(1).copied().unwrap_or(0);
    let is_abs = d0 == path_sep || d0 == alt_path_sep || (d0 != 0 && d1 == b':');
    if is_abs {
        return 0;
    }
    let last_parent = parent_dir[parent_dir.len() - 1];
    if last_parent == path_sep || last_parent == alt_path_sep {
        1
    } else {
        2
    }
}

/// `Options::InitOptions`: normalizes and clamps `MemoryPressureInterval` and
/// the `Dupe*` integer options.
#[allow(clippy::too_many_arguments)]
pub fn options_normalize_dupe_and_mem_options(
    mem_pressure_interval: i32,
    dupe_stream_timeout: i32,
    dupe_search_delay: i32,
    dupe_search_max_donors: i32,
    dupe_health_percent: i32,
    dupe_health_min: i32,
    dupe_health_max: i32,
    dupe_body_checks: i32,
    dupe_health_budget: i32,
    dupe_min_alive: i32,
    dupe_fast_donors: i32,
) -> (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32) {
    let norm_mem = if mem_pressure_interval > 0 {
        mem_pressure_interval
    } else {
        500
    };
    let norm_stream_timeout = dupe_stream_timeout.max(0);
    let norm_search_delay = dupe_search_delay.max(0);
    let norm_max_donors = dupe_search_max_donors.max(0);
    let norm_health_pct = dupe_health_percent.max(1).min(100);
    let norm_health_min = dupe_health_min.max(1);
    let norm_health_max = dupe_health_max.max(norm_health_min);
    let norm_body_checks = dupe_body_checks.max(0);
    let norm_health_budget = dupe_health_budget.max(5);
    let norm_min_alive = dupe_min_alive.max(0).min(100);
    let norm_fast_donors = dupe_fast_donors.max(0);
    (
        norm_mem,
        norm_stream_timeout,
        norm_search_delay,
        norm_max_donors,
        norm_health_pct,
        norm_health_min,
        norm_health_max,
        norm_body_checks,
        norm_health_budget,
        norm_min_alive,
        norm_fast_donors,
    )
}

/// Normalized `DupePrimaryScore`, `DupeSizeTolerance`, `DupeBodyPercent`,
/// `DupeCheckConcurrency` and `DupeCheckConnections`.
pub fn options_normalize_dupe_check_options(
    primary_score: i32,
    size_tolerance: i32,
    body_percent: i32,
    check_concurrency: i32,
    check_connections: i32,
) -> (i32, i32, i32, i32, i32) {
    (
        primary_score.max(1000),
        size_tolerance.clamp(0, 1000),
        body_percent.clamp(0, 100),
        check_concurrency.clamp(1, 100),
        check_connections.clamp(1, 1000),
    )
}

/// `Options::InitServers`: classifies server option definition completeness.
pub fn options_check_server_def(
    has_any_field: bool,
    has_host: bool,
    has_port: bool,
    has_connections: bool,
) -> i32 {
    if !has_any_field {
        0
    } else if has_host && has_port && has_connections {
        1
    } else {
        2
    }
}

/// `Options::InitCategories` & `Options::InitFeeds`: classifies category or feed
/// definition completeness.
pub fn options_check_category_or_feed_def(has_any_field: bool, has_required: bool) -> i32 {
    if !has_any_field {
        0
    } else if has_required {
        1
    } else {
        2
    }
}

/// `Options::CheckOptions`: reconciles `DirectWrite` and `DirectRename`.
pub fn options_reconcile_write_flags(
    raw_article: bool,
    skip_write: bool,
    direct_write: bool,
    direct_rename: bool,
) -> (bool, bool) {
    let out_direct_write = if raw_article { false } else { direct_write };
    let out_direct_rename = if skip_write { false } else { direct_rename };
    (out_direct_write, out_direct_rename)
}

/// Rule 7.1 in `Options::CheckOptions`: returns 1 when `ServerX.Encryption` is disabled
/// while `ServerX.Cipher` is non-empty.
pub unsafe fn roc_options_server_cipher_without_tls(
    tls_enabled: c_int,
    cipher: *const c_char,
) -> c_int {
    if tls_enabled == 0 && !cipher.is_null() && unsafe { *cipher != 0 } {
        1
    } else {
        0
    }
}

/// Evaluates Rules 7.2, 7.3, and 7.4 in `Options::CheckOptions`.
pub unsafe fn roc_options_reconcile_conflict_rules(
    dupe_search: c_int,
    health_check: c_int,
    hc_park: c_int,
    flush_queue: c_int,
    direct_write: c_int,
    unpack_pause_queue: c_int,
    direct_unpack: c_int,
    out_disable_dupe_search: *mut c_int,
    out_enable_flush_queue: *mut c_int,
    out_enforce_unpack_mutex: *mut c_int,
) {
    let disable_dupe = if dupe_search != 0 && health_check == hc_park {
        1
    } else {
        0
    };
    let enable_flush = if flush_queue == 0 && direct_write != 0 {
        1
    } else {
        0
    };
    let enforce_mutex = if unpack_pause_queue == 0 && direct_unpack != 0 {
        1
    } else {
        0
    };
    if !out_disable_dupe_search.is_null() {
        unsafe { *out_disable_dupe_search = disable_dupe };
    }
    if !out_enable_flush_queue.is_null() {
        unsafe { *out_enable_flush_queue = enable_flush };
    }
    if !out_enforce_unpack_mutex.is_null() {
        unsafe { *out_enforce_unpack_mutex = enforce_mutex };
    }
}

/// Evaluates `DirectClientMode` in `NZBGet::BootConfig`.
pub fn roc_nzbget_boot_is_direct_client_mode(
    test_backtrace: c_int,
    web_get: c_int,
    sig_verify: c_int,
    client_op: c_int,
    op_no_operation: c_int,
    print_options: c_int,
) -> c_int {
    if test_backtrace != 0
        || web_get != 0
        || sig_verify != 0
        || client_op != op_no_operation
        || print_options != 0
    {
        1
    } else {
        0
    }
}

/// Returns 1 when `NZBGet::BootConfig` should pause all activities due to configuration errors.
pub fn roc_nzbget_boot_should_pause_on_config_errors(
    config_errors: c_int,
    client_op: c_int,
    op_no_operation: c_int,
) -> c_int {
    if config_errors != 0 && client_op == op_no_operation {
        1
    } else {
        0
    }
}

/// Computes the clamped debounce offset in milliseconds from `g_reloadFirstRequestTime`
/// during burst reload requests in `Reload()`.
pub fn roc_nzbget_reload_clamp_debounce_ms(
    elapsed_since_first_ms: i64,
    debounce_delay_ms: i64,
    max_burst_delay_ms: i64,
) -> i64 {
    let candidate = elapsed_since_first_ms.saturating_add(debounce_delay_ms);
    if candidate < max_burst_delay_ms {
        candidate
    } else {
        max_burst_delay_ms
    }
}

/// Checks whether `umask_val` is a valid octal mask (`uMask < 01000`) in `NZBGet::Phase1_Init`.
pub fn nzbget_is_valid_umask(umask_val: u32) -> bool {
    umask_val < 0o1000
}

/// Determines whether `NZBGet::StartRemoteServer` should start the secure control server.
pub fn nzbget_should_start_secure_server(
    secure_control: bool,
    is_unix: bool,
    control_ip: &[u8],
) -> bool {
    if !secure_control {
        return false;
    }
    if is_unix && !control_ip.is_empty() && control_ip[0] == b'/' {
        return false;
    }
    true
}

/// Determines whether `NZBGet::DoMainLoop` should stop standalone coordinators.
pub fn nzbget_should_stop_standalone(
    server_mode: bool,
    queue_more: bool,
    url_more: bool,
    post_more: bool,
) -> bool {
    !server_mode && !queue_more && !url_more && !post_more
}

/// Computes the exit code for `NZBGet::ProcessWebGet` (`status == ad_finished ? 0 : 1`).
pub fn nzbget_webget_exit_code(status: i32, ad_finished: i32) -> i32 {
    if status == ad_finished {
        0
    } else {
        1
    }
}

/// Computes the exit code for `NZBGet::ProcessSigVerify` (`ok ? 93 : 1`).
pub fn nzbget_sigverify_exit_code(ok: bool) -> i32 {
    if ok {
        93
    } else {
        1
    }
}

/// Finds the first `${Var}` reference in `s` for `Options::SetOption`.
pub fn options_find_var_reference(s: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 1 < s.len() {
        if s[i] == b'$' && s[i + 1] == b'{' {
            let rest = &s[i + 2..];
            let end_offset = rest.iter().position(|&b| b == b'}')?;
            return Some((i, end_offset));
        }
        i += 1;
    }
    None
}

/// Strips a single trailing `\n` and then a single trailing `\r` from `line[..len]`
/// and checks whether the line should be skipped.
pub fn options_trim_and_check_config_line(line: &[u8]) -> (usize, bool) {
    let mut len = line.len();
    if len > 0 && line[len - 1] == b'\n' {
        len -= 1;
    }
    if len > 0 && line[len - 1] == b'\r' {
        len -= 1;
    }
    let trimmed = &line[..len];
    let skip = trimmed.is_empty() || trimmed[0] == b'#' || trimmed.iter().all(|&b| b == b' ');
    (len, skip)
}

pub const POSSIBLE_CONFIG_LOCATIONS: &[&[u8]] = &[
    b"~/.rustyget",
    b"~/.config/rustyget.conf",
    b"~/.config/rustyget/rustyget.conf",
    b"/etc/rustyget.conf",
    b"/usr/etc/rustyget.conf",
    b"/usr/local/etc/rustyget.conf",
    b"/opt/etc/rustyget.conf",
    b"~/usr/etc/rustyget.conf",
    b"~/.nzbget",
    b"~/.config/nzbget.conf",
    b"~/.config/nzbget/nzbget.conf",
    b"/etc/nzbget.conf",
    b"/usr/etc/nzbget.conf",
    b"/usr/local/etc/nzbget.conf",
    b"/opt/etc/nzbget.conf",
    b"~/usr/etc/nzbget.conf",
];

/// Searches for a default configuration file in `app_dir` first, then in `POSSIBLE_CONFIG_LOCATIONS`
/// (with `~` expanded via `HOME` or passwd fallback).
pub fn find_default_config_file(
    app_dir: &[u8],
    sep: u8,
    mut file_exists: impl FnMut(&CStr) -> bool,
) -> Option<Vec<u8>> {
    let first = options_format_default_conf_path(app_dir, sep);
    let first_c = cstring(first.clone());
    if file_exists(&first_c) {
        return Some(first);
    }

    for &alt in POSSIBLE_CONFIG_LOCATIONS {
        let alt_c = cstring(alt.to_vec());
        let expanded = crate::fsops::expand_home_path_auto(Some(&alt_c));
        let exp_c = cstring(expanded.clone());
        if file_exists(&exp_c) {
            return Some(expanded);
        }
    }
    None
}

/// Reads a configuration file line by line, trimming CR/LF and skipping blank/comment lines,
/// invoking `on_line(line_no, trimmed_line)` for each non-skipped line (`Options::LoadConfigFile`).
pub fn load_config_file_lines(path: &CStr, mut on_line: impl FnMut(i32, &CStr)) -> bool {
    #[cfg(unix)]
    let os_path = {
        use std::os::unix::ffi::OsStrExt;
        std::path::Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()))
    };
    #[cfg(not(unix))]
    let os_path = std::path::Path::new(path.to_str().unwrap_or(""));

    let Ok(file) = std::fs::File::open(os_path) else {
        return false;
    };
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(file);
    let mut raw = Vec::new();
    let mut line_no = 0i32;
    loop {
        raw.clear();
        match reader.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                line_no += 1;
                if let Some(nul) = raw.iter().position(|&b| b == 0) {
                    raw.truncate(nul);
                }
                let (trimmed_len, skip) = options_trim_and_check_config_line(&raw);
                if skip {
                    continue;
                }
                let c_line = cstring(raw[..trimmed_len].to_vec());
                on_line(line_no, &c_line);
            }
        }
    }
    true
}

const SCHEDULER_COMMAND_MAP: [(&[u8], i32); 26] = [
    (b"pausedownload", 0),
    (b"pause", 0),
    (b"unpausedownload", 1),
    (b"resumedownload", 1),
    (b"unpause", 1),
    (b"resume", 1),
    (b"pausepostprocess", 2),
    (b"unpausepostprocess", 3),
    (b"resumepostprocess", 3),
    (b"pausepost", 2),
    (b"unpausepost", 3),
    (b"resumepost", 3),
    (b"downloadrate", 4),
    (b"setdownloadrate", 4),
    (b"rate", 4),
    (b"speed", 4),
    (b"process", 6),
    (b"pausescan", 7),
    (b"unpausescan", 8),
    (b"resumescan", 8),
    (b"activateserver", 9),
    (b"activateservers", 9),
    (b"deactivateserver", 10),
    (b"deactivateservers", 10),
    (b"fetchfeed", 11),
    (b"fetchfeeds", 11),
];

/// Case-insensitively maps a scheduler command string to `Options::ESchedulerCommand` (`0..=11`),
/// or `-1` if unknown.
pub fn options_lookup_scheduler_command(cmd: &[u8]) -> i32 {
    for &(name, val) in &SCHEDULER_COMMAND_MAP {
        if cmd.eq_ignore_ascii_case(name) {
            return val;
        }
    }
    -1
}

/// Validates `TaskX.Param` for `Options::InitScheduler`.
pub fn options_validate_scheduler_param(
    task_command: i32,
    param: Option<&[u8]>,
    split_cmd_empty: bool,
) -> i32 {
    if let Some(p) = param {
        if !p.is_empty() && task_command == 6 && split_cmd_empty {
            return 1;
        }
    }

    if task_command == 4 {
        let Some(p) = param else {
            return 3;
        };
        let mut idx = 0;
        while idx < p.len() && matches!(p[idx], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            idx += 1;
        }
        let rest = &p[idx..];
        if !rest.is_empty() {
            let (neg, digits) = if rest[0] == b'-' {
                (true, &rest[1..])
            } else if rest[0] == b'+' {
                (false, &rest[1..])
            } else {
                (false, rest)
            };
            if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
                return 2;
            }
            let mut val: std::os::raw::c_long = 0;
            for &d in digits {
                let digit = (d - b'0') as std::os::raw::c_long;
                val = val.saturating_mul(10);
                val = if neg {
                    val.saturating_sub(digit)
                } else {
                    val.saturating_add(digit)
                };
            }
            if (val as i32) < 0 {
                return 2;
            }
        }
    }

    if matches!(task_command, 5 | 6 | 9 | 10 | 11) {
        let is_empty = match param {
            None => true,
            Some(p) => p.is_empty(),
        };
        if is_empty {
            return 3;
        }
    }

    0
}

const BOOL_NAMES: [&CStr; 12] = [
    c"yes",
    c"no",
    c"true",
    c"false",
    c"1",
    c"0",
    c"on",
    c"off",
    c"enable",
    c"disable",
    c"enabled",
    c"disabled",
];
const BOOL_VALUES: [i32; 12] = [1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0];

const CERT_VERIF_NAMES: [&CStr; 3] = [c"none", c"minimal", c"strict"];
const CERT_VERIF_VALUES: [i32; 3] = [0, 1, 2];

const IP_VERSION_NAMES: [&CStr; 3] = [c"auto", c"ipv4", c"ipv6"];
const IP_VERSION_VALUES: [i32; 3] = [0, 4, 6];

const ARCHIVE_ACTION_NAMES: [&CStr; 2] = [c"move", c"delete"];
const ARCHIVE_ACTION_VALUES: [i32; 2] = [1, 0];

const OUTPUT_MODE_NAMES: [&CStr; 7] = [
    c"loggable",
    c"logable",
    c"log",
    c"colored",
    c"color",
    c"ncurses",
    c"curses",
];
const OUTPUT_MODE_VALUES: [i32; 7] = [0, 0, 0, 1, 1, 2, 2];

const PAR_CHECK_NAMES: [&CStr; 4] = [c"auto", c"always", c"force", c"manual"];
const PAR_CHECK_VALUES: [i32; 4] = [0, 1, 2, 3];

const PAR_SCAN_NAMES: [&CStr; 4] = [c"limited", c"extended", c"full", c"dupe"];
const PAR_SCAN_VALUES: [i32; 4] = [0, 1, 2, 3];

const DUPE_ARTICLE_FALLBACK_NAMES: [&CStr; 5] = [c"no", c"article", c"stream", c"live", c"yes"];
const DUPE_ARTICLE_FALLBACK_VALUES: [i32; 5] = [0, 1, 2, 3, 1];

const POST_STRATEGY_NAMES: [&CStr; 4] = [c"sequential", c"balanced", c"aggressive", c"rocket"];
const POST_STRATEGY_VALUES: [i32; 4] = [0, 1, 2, 3];

const FILE_NAMING_NAMES: [&CStr; 3] = [c"auto", c"article", c"nzb"];
const FILE_NAMING_VALUES: [i32; 3] = [0, 1, 2];

const HEALTH_CHECK_NAMES: [&CStr; 5] = [c"pause", c"delete", c"park", c"none", c"dupe"];
const HEALTH_CHECK_VALUES: [i32; 5] = [0, 1, 2, 3, 4];

const TARGET_NAMES: [&CStr; 4] = [c"screen", c"log", c"both", c"none"];
const TARGET_VALUES: [i32; 4] = [1, 2, 3, 0];

const WRITE_LOG_NAMES: [&CStr; 4] = [c"none", c"append", c"reset", c"rotate"];
const WRITE_LOG_VALUES: [i32; 4] = [0, 1, 2, 3];

const COMMAND_NAMES: [&CStr; 26] = [
    c"pausedownload",
    c"pause",
    c"unpausedownload",
    c"resumedownload",
    c"unpause",
    c"resume",
    c"pausepostprocess",
    c"unpausepostprocess",
    c"resumepostprocess",
    c"pausepost",
    c"unpausepost",
    c"resumepost",
    c"downloadrate",
    c"setdownloadrate",
    c"rate",
    c"speed",
    c"process",
    c"pausescan",
    c"unpausescan",
    c"resumescan",
    c"activateserver",
    c"activateservers",
    c"deactivateserver",
    c"deactivateservers",
    c"fetchfeed",
    c"fetchfeeds",
];
const COMMAND_VALUES: [i32; 26] = [
    0, 0, 1, 1, 1, 1, 2, 3, 3, 2, 3, 3, 4, 4, 4, 4, 6, 7, 8, 8, 9, 9, 10, 10, 11, 11,
];

/// Emits all default option `(name, value)` pairs for `Options::InitDefaults`.
pub fn init_defaults_with(disable_curses: bool, mut set_opt: impl FnMut(&CStr, &CStr)) {
    let output_mode = if disable_curses { c"color" } else { c"curses" };
    let defaults: [(&CStr, &CStr); 139] = [
        (c"MainDir", c"~/downloads"),
        (c"WebDir", c""),
        (c"ConfigTemplate", c""),
        (c"TempDir", c"${MainDir}/tmp"),
        (c"DestDir", c"${MainDir}/dst"),
        (c"InterDir", c"${MainDir}/inter"),
        (c"QueueDir", c"${MainDir}/queue"),
        (c"NzbDir", c"${MainDir}/nzb"),
        (c"LockFile", c"${MainDir}/rustyget.lock"),
        (c"LogFile", c"${MainDir}/rustyget.log"),
        (c"RequiredDir", c""),
        (c"WriteLog", c"append"),
        (c"RotateLog", c"3"),
        (c"AppendCategoryDir", c"yes"),
        (c"OutputMode", output_mode),
        (c"DupeCheck", c"yes"),
        (c"DupeArticleFallback", c"no"),
        (c"DupeStreamDecompress", c"no"),
        (c"DupeStreamTimeout", c"60"),
        (c"DupeSearch", c"no"),
        (c"DupeSearchDryRun", c"no"),
        (c"DupeSearchUrl", c""),
        (c"DupeSearchApiKey", c""),
        (c"DupeSearchDelay", c"20"),
        (c"DupeSearchMaxDonors", c"0"),
        (c"DupeHealthPercent", c"5"),
        (c"DupeHealthMin", c"50"),
        (c"DupeHealthMax", c"1000"),
        (c"DupeBodyChecks", c"20"),
        (c"DupeHealthBudget", c"120"),
        (c"DupeMinAlive", c"50"),
        (c"DupeFastDonors", c"5"),
        (c"DupePrimaryScore", c"1000000"),
        (c"DupeSizeTolerance", c"0"),
        (c"DupeBodyPercent", c"20"),
        (c"DupeCheckConcurrency", c"10"),
        (c"DupeCheckConnections", c"20"),
        (c"DownloadRate", c"0"),
        (c"ControlIP", c"0.0.0.0"),
        (c"ControlUsername", c"rustyget"),
        (c"ControlPassword", c"tegbzn6789"),
        (c"RestrictedUsername", c""),
        (c"RestrictedPassword", c""),
        (c"AddUsername", c""),
        (c"AddPassword", c""),
        (c"ControlPort", c"6789"),
        (c"FormAuth", c"no"),
        (c"JsonP", c"no"),
        (c"SecureControl", c"no"),
        (c"SecurePort", c"6791"),
        (c"SecureCert", c""),
        (c"SecureKey", c""),
        (c"CertStore", c""),
        (c"CertCheck", c"no"),
        (c"AuthorizedIP", c""),
        (c"ArticleTimeout", c"60"),
        (c"ArticleReadChunkSize", c"4"),
        (c"UrlTimeout", c"60"),
        (c"RemoteTimeout", c"90"),
        (c"FlushQueue", c"yes"),
        (c"SystemHealthCheck", c"yes"),
        (c"NzbLog", c"yes"),
        (c"RawArticle", c"no"),
        (c"SkipWrite", c"no"),
        (c"ArticleRetries", c"3"),
        (c"ArticleInterval", c"10"),
        (c"UrlRetries", c"3"),
        (c"UrlInterval", c"10"),
        (c"ContinuePartial", c"yes"),
        (c"UrlConnections", c"4"),
        (c"LogBuffer", c"1000"),
        (c"InfoTarget", c"both"),
        (c"WarningTarget", c"both"),
        (c"ErrorTarget", c"both"),
        (c"DebugTarget", c"none"),
        (c"DetailTarget", c"both"),
        (c"ParCheck", c"auto"),
        (c"ParRepair", c"yes"),
        (c"ParScan", c"extended"),
        (c"ParQuick", c"yes"),
        (c"PostStrategy", c"sequential"),
        (c"FileNaming", c"article"),
        (c"PostDownloadRename", c"yes"),
        (c"RenameAfterUnpack", c"yes"),
        (c"RenameIgnoreExt", c""),
        (c"ParRename", c"yes"),
        (c"ParBuffer", c"250"),
        (c"ParThreads", c"0"),
        (c"RarRename", c"yes"),
        (c"HealthCheck", c"none"),
        (c"DirectRename", c"yes"),
        (c"HardLinking", c"no"),
        (c"HardLinkingIgnoreExt", c".zip, .7z, .rar, *.7z.###, *.r##"),
        (c"DaemonUsername", c"root"),
        (c"UMask", c"1000"),
        (c"UpdateInterval", c"200"),
        (c"CursesNzbName", c"yes"),
        (c"CursesTime", c"no"),
        (c"CursesGroup", c"no"),
        (c"CrcCheck", c"yes"),
        (c"DirectWrite", c"yes"),
        (c"WriteBuffer", c"1024"),
        (c"NzbDirInterval", c"5"),
        (c"NzbDirFileAge", c"60"),
        (c"NzbDirArchiveScan", c"yes"),
        (c"NzbDirArchiveAction", c"delete"),
        (c"DiskSpace", c"250"),
        (c"CrashTrace", c"no"),
        (c"CrashDump", c"no"),
        (c"ParPauseQueue", c"no"),
        (c"ScriptPauseQueue", c"no"),
        (c"NzbCleanupDisk", c"yes"),
        (c"ParTimeLimit", c"0"),
        (c"KeepHistory", c"7"),
        (c"Unpack", c"yes"),
        (c"DirectUnpack", c"yes"),
        (c"UseTempUnpackDir", c"yes"),
        (c"UnpackCleanupDisk", c"yes"),
        (c"UnrarCmd", c""),
        (c"SevenZipCmd", c""),
        (c"UnpackPassFile", c""),
        (c"UnpackPauseQueue", c"no"),
        (c"ExtCleanupDisk", c".par2, .sfv, _brokenlog.txt"),
        (c"ParIgnoreExt", c""),
        (c"UnpackIgnoreExt", c""),
        (c"FeedHistory", c"7"),
        (c"UrlForce", c"yes"),
        (c"TimeCorrection", c"0"),
        (c"PropagationDelay", c"0"),
        (c"ArticleCache", c"500"),
        (c"MemoryPressureInterval", c"500"),
        (c"MemoryPressurePath", c""),
        (c"EventInterval", c"0"),
        (c"ShellOverride", c""),
        (c"MonthlyQuota", c"0"),
        (c"QuotaStartDay", c"1"),
        (c"DailyQuota", c"0"),
        (c"ReorderFiles", c"yes"),
        (c"UpdateCheck", c"none"),
    ];
    for (name, val) in defaults {
        set_opt(name, val);
    }
}

/// Expands `${Var}` references in `initial_value` using `get_option` (`Options::SetOption`).
pub fn expand_option_variables(
    initial_value: &[u8],
    mut get_option: impl FnMut(&CStr) -> Option<Vec<u8>>,
) -> Vec<u8> {
    let mut curvalue = initial_value.to_vec();
    while let Some((dollar_pos, var_len)) = options_find_var_reference(&curvalue) {
        let copy_len = var_len.min(99);
        let var_name = cstring(curvalue[dollar_pos + 2..dollar_pos + 2 + copy_len].to_vec());
        if let Some(var_val) = get_option(&var_name) {
            let end = dollar_pos + 2 + var_len + 1;
            curvalue.splice(dollar_pos..end, var_val);
        } else {
            break;
        }
    }
    curvalue
}

/// Splits an `option=value` string and normalizes legacy option names/values (`Options::SplitOptionString`).
pub fn split_and_convert_option_string(option: &CStr) -> Option<(CString, CString)> {
    let (raw_name, raw_val) = options_split_option_string(option.to_bytes())?;
    let name_cstr = cstring(raw_name.to_vec());
    let val_cstr = cstring(raw_val.to_vec());
    Some(convert_old_option(&name_cstr, &val_cstr))
}

/// Resolves a tool command option (`UnrarCmd` / `SevenZipCmd`) to a tool path (`Options::SetToolPathOption`).
pub fn resolve_tool_path_option(value: &[u8]) -> Vec<u8> {
    if value.is_empty() {
        return Vec::new();
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let direct_path = std::path::Path::new(std::ffi::OsStr::from_bytes(value));
        if direct_path.exists() && direct_path.is_file() {
            return value.to_vec();
        }
        let args = crate::util::split_command_line(value);
        let tool = args.first().map(|v| v.as_slice()).unwrap_or(value);
        let direct_tool = std::path::Path::new(std::ffi::OsStr::from_bytes(tool));
        if direct_tool
            .parent()
            .is_some_and(|p| !p.as_os_str().is_empty())
            && direct_tool.exists()
            && direct_tool.is_file()
        {
            return tool.to_vec();
        }
        if let Some(env_path) = std::env::var_os("PATH") {
            if let Some(resolved) =
                crate::util::util_resolve_path_from_env(tool, env_path.as_bytes())
            {
                return resolved;
            }
        }
        tool.to_vec()
    }
    #[cfg(not(unix))]
    {
        let args = crate::util::split_command_line(value);
        args.into_iter().next().unwrap_or_else(|| value.to_vec())
    }
}

/// Checks whether any wildcard alias in `aliases` matches `name` (`Options::Categories::FindCategory`).
pub fn category_alias_list_matches(aliases: &[&[u8]], name: &[u8], _signed_char: bool) -> bool {
    let lower = crate::wildmask::Lower::Ascii;
    aliases
        .iter()
        .any(|alias| crate::wildmask::wild_match(&lower, alias, name, None))
}

/// All scalar integer, boolean, and enum fields parsed by `Options::InitOptions` and `Options::CheckOptions`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NzbgetRsParsedOptions {
    pub download_rate: i32,
    pub article_timeout: i32,
    pub article_read_chunk_size: i32,
    pub url_timeout: i32,
    pub remote_timeout: i32,
    pub article_retries: i32,
    pub article_interval: i32,
    pub url_retries: i32,
    pub url_interval: i32,
    pub control_port: i32,
    pub secure_port: i32,
    pub url_connections: i32,
    pub log_buffer: i32,
    pub rotate_log: i32,
    pub umask: i32,
    pub update_interval: i32,
    pub write_buffer: i32,
    pub nzb_dir_interval: i32,
    pub nzb_dir_file_age: i32,
    pub nzb_dir_archive_scan: i32,
    pub nzb_dir_archive_action: i32,
    pub disk_space: i32,
    pub par_time_limit: i32,
    pub keep_history: i32,
    pub feed_history: i32,
    pub time_correction: i32,
    pub propagation_delay: i32,
    pub article_cache: i32,
    pub memory_pressure_interval: i32,
    pub event_interval: i32,
    pub par_buffer: i32,
    pub par_threads: i32,
    pub monthly_quota: i32,
    pub quota_start_day: i32,
    pub daily_quota: i32,
    pub system_health_check: i32,
    pub nzb_log: i32,
    pub append_category_dir: i32,
    pub continue_partial: i32,
    pub flush_queue: i32,
    pub dupe_check: i32,
    pub par_repair: i32,
    pub par_quick: i32,
    pub par_rename: i32,
    pub rar_rename: i32,
    pub direct_rename: i32,
    pub hard_linking: i32,
    pub curses_nzb_name: i32,
    pub curses_time: i32,
    pub curses_group: i32,
    pub crc_check: i32,
    pub direct_write: i32,
    pub raw_article: i32,
    pub skip_write: i32,
    pub crash_trace: i32,
    pub crash_dump: i32,
    pub par_pause_queue: i32,
    pub script_pause_queue: i32,
    pub nzb_cleanup_disk: i32,
    pub form_auth: i32,
    pub json_p: i32,
    pub secure_control: i32,
    pub unpack: i32,
    pub direct_unpack: i32,
    pub use_temp_unpack_dir: i32,
    pub unpack_cleanup_disk: i32,
    pub unpack_pause_queue: i32,
    pub unpack_mutex_enforced: i32,
    pub url_force: i32,
    pub cert_check: i32,
    pub reorder_files: i32,
    pub post_download_rename: i32,
    pub rename_after_unpack: i32,
    pub output_mode: i32,
    pub par_check: i32,
    pub par_scan: i32,
    pub dupe_article_fallback: i32,
    pub dupe_stream_decompress: i32,
    pub dupe_stream_timeout: i32,
    pub dupe_search: i32,
    pub dupe_search_dry_run: i32,
    pub dupe_search_delay: i32,
    pub dupe_search_max_donors: i32,
    pub dupe_health_percent: i32,
    pub dupe_health_min: i32,
    pub dupe_health_max: i32,
    pub dupe_body_checks: i32,
    pub dupe_health_budget: i32,
    pub dupe_min_alive: i32,
    pub dupe_fast_donors: i32,
    pub dupe_primary_score: i32,
    pub dupe_size_tolerance: i32,
    pub dupe_body_percent: i32,
    pub dupe_check_concurrency: i32,
    pub dupe_check_connections: i32,
    pub post_strategy: i32,
    pub file_naming: i32,
    pub health_check: i32,
    pub info_target: i32,
    pub warning_target: i32,
    pub error_target: i32,
    pub debug_target: i32,
    pub detail_target: i32,
    pub write_log: i32,
    pub tls: i32,
}

impl NzbgetRsParsedOptions {
    /// Returns the startup scalar defaults matching historical `Options` member initializers
    /// so pre-`InitOptions` diagnostics buffer to `mtScreen` before `Log::InitOptions()`.
    pub fn with_defaults() -> Self {
        Self {
            info_target: 1,
            warning_target: 1,
            error_target: 1,
            debug_target: 0,
            detail_target: 1,
            system_health_check: 1,
            article_read_chunk_size: 4,
            dupe_stream_timeout: 60,
            dupe_search_delay: 20,
            dupe_health_percent: 5,
            dupe_health_min: 50,
            dupe_health_max: 1000,
            dupe_body_checks: 20,
            dupe_health_budget: 120,
            dupe_min_alive: 50,
            dupe_fast_donors: 5,
            dupe_primary_score: 1_000_000,
            dupe_size_tolerance: 0,
            dupe_body_percent: 20,
            dupe_check_concurrency: 10,
            dupe_check_connections: 20,
            output_mode: 0,
            write_log: 1,
            par_check: 3,
            par_scan: 0,
            par_quick: 1,
            post_strategy: 0,
            health_check: 3,
            nzb_dir_archive_scan: 1,
            nzb_dir_archive_action: 1,
            use_temp_unpack_dir: 1,
            memory_pressure_interval: 500,
            file_naming: 1,
            post_download_rename: 1,
            rename_after_unpack: 1,
            ..Self::default()
        }
    }
}

fn format_bytes_msg(parts: &[&[u8]]) -> CString {
    let total_len: usize = parts.iter().map(|p| p.len()).sum();
    let mut out = Vec::with_capacity(total_len);
    for part in parts {
        out.extend_from_slice(part);
    }
    cstring(out)
}

/// Host abstraction shared by C++ `Options` (via FFI callbacks) and pure-Rust `RustOptions`.
pub trait OptionsHost {
    fn find_entry(&mut self, name: &CStr) -> Option<(Option<CString>, Option<CString>, i32)>;
    fn set_entry_raw_value(&mut self, name: &CStr, value: &CStr);
    fn set_option(&mut self, name: &CStr, value: &CStr);
    fn set_config_line(&mut self, line: i32);
    fn get_config_line(&self) -> i32;
    fn config_error(&mut self, msg: &CStr);
    fn config_warn(&mut self, msg: &CStr);
    fn file_exists(&self, path: &CStr) -> bool;
    fn ensure_directory(&mut self, dir: &CStr) -> Result<(), CString>;
    #[allow(clippy::too_many_arguments)]
    fn add_news_server(
        &mut self,
        id: i32,
        active: bool,
        name: Option<&CStr>,
        host: Option<&CStr>,
        port: i32,
        ip_version: i32,
        user: Option<&CStr>,
        pass: Option<&CStr>,
        join_group: bool,
        tls: bool,
        cipher: Option<&CStr>,
        max_connections: i32,
        retention: i32,
        level: i32,
        group: i32,
        optional: bool,
        cert_verif_level: u32,
    );
    #[allow(clippy::too_many_arguments)]
    fn add_feed(
        &mut self,
        id: i32,
        name: Option<&CStr>,
        url: Option<&CStr>,
        interval: i32,
        filter: Option<&CStr>,
        backlog: bool,
        pause_nzb: bool,
        category: Option<&CStr>,
        category_source: i32,
        priority: i32,
        cert_verif_level: u32,
    );
    fn add_task(
        &mut self,
        id: i32,
        hours: i32,
        minutes: i32,
        week_days_bits: i32,
        command: i32,
        param: Option<&CStr>,
    );
    fn add_category(
        &mut self,
        name: &CStr,
        dest_dir: Option<&CStr>,
        unpack: bool,
        aliases: &[CString],
    );

    fn get_option(&mut self, name: &CStr) -> Option<CString> {
        let (val_opt, _def_opt, line_no) = self.find_entry(name)?;
        let next_line = options_update_config_line_on_get(line_no, self.get_config_line());
        self.set_config_line(next_line);
        val_opt
    }

    fn locate_option_src_pos(&mut self, name: &CStr) {
        let line = match self.find_entry(name) {
            Some((_v, _d, l)) => options_resolve_locate_src_line(true, l),
            None => options_resolve_locate_src_line(false, 0),
        };
        self.set_config_line(line);
    }

    fn parse_enum_value(&mut self, opt_name: &CStr, argn: &[&CStr], argv: &[i32]) -> i32 {
        let default_ret = argv.first().copied().unwrap_or(0);
        let Some((val_opt, def_opt, line_no)) = self.find_entry(opt_name) else {
            let msg =
                format_bytes_msg(&[b"Undefined value for option \"", opt_name.to_bytes(), b"\""]);
            self.config_error(&msg);
            return default_ret;
        };
        let val = val_opt.as_deref().unwrap_or(c"");
        let def_val = def_opt.as_deref().unwrap_or(c"");
        let (ok, parsed_val, norm_idx) = parse_enum_value(val, def_val, argn, argv);
        if ok {
            if let Some(idx) = norm_idx {
                if idx < argn.len() {
                    self.set_entry_raw_value(opt_name, argn[idx]);
                }
            }
            return parsed_val;
        }
        self.set_config_line(line_no);
        let msg = format_bytes_msg(&[
            b"Invalid value for option \"",
            opt_name.to_bytes(),
            b"\": \"",
            val.to_bytes(),
            b"\"",
        ]);
        self.config_error(&msg);
        if let Some(idx) = norm_idx {
            if idx < argn.len() {
                self.set_entry_raw_value(opt_name, argn[idx]);
            }
        }
        parsed_val
    }

    fn parse_int_value(&mut self, opt_name: &CStr, base: i32) -> i32 {
        let Some((val_opt, def_opt, line_no)) = self.find_entry(opt_name) else {
            let msg =
                format_bytes_msg(&[b"Undefined value for option \"", opt_name.to_bytes(), b"\""]);
            self.config_error(&msg);
            return 0;
        };
        let val = val_opt.as_deref().unwrap_or(c"");
        let def_val = def_opt.as_deref().unwrap_or(c"");
        let (ok, parsed) = parse_int_value(val, def_val, base);
        if !ok {
            self.set_config_line(line_no);
            let msg = format_bytes_msg(&[
                b"Invalid value for option \"",
                opt_name.to_bytes(),
                b"\": \"",
                val.to_bytes(),
                b"\"",
            ]);
            self.config_error(&msg);
            self.set_entry_raw_value(opt_name, def_val);
        }
        parsed
    }

    fn parse_bool_value(&mut self, opt_name: &CStr) -> bool {
        self.parse_enum_value(opt_name, &BOOL_NAMES, &BOOL_VALUES) != 0
    }
}

/// Implements `Options::CheckDir` on top of `OptionsHost`.
pub fn check_dir_host<H: OptionsHost + ?Sized>(
    host: &mut H,
    no_disk_access: bool,
    option_name: &CStr,
    parent_dir: Option<&CStr>,
    allow_empty: bool,
    create: bool,
) -> Option<CString> {
    let tempdir = host.get_option(option_name);
    if no_disk_access {
        return tempdir;
    }
    let is_empty = tempdir.as_ref().is_none_or(|s| s.to_bytes().is_empty());
    if is_empty {
        if !allow_empty {
            let msg = format_bytes_msg(&[
                b"Invalid value for option \"",
                option_name.to_bytes(),
                b"\": <empty>",
            ]);
            host.config_error(&msg);
        }
        return Some(c"".to_owned());
    }

    let mut dir_bytes = tempdir.unwrap().into_bytes();
    crate::paths::normalize_path_separators(&mut dir_bytes);
    let stripped_len = options_strip_trailing_sep(&dir_bytes, crate::paths::PATH_SEPARATOR);
    dir_bytes.truncate(stripped_len);

    let parent_bytes = parent_dir.map(|p| p.to_bytes()).unwrap_or(&[]);
    let join_mode = options_check_dir_join_mode(
        &dir_bytes,
        parent_bytes,
        crate::paths::PATH_SEPARATOR,
        crate::paths::ALT_PATH_SEPARATOR,
    );
    if join_mode != 0 {
        let mut joined = options_format_joined_dir(
            parent_bytes,
            crate::paths::PATH_SEPARATOR,
            &dir_bytes,
            join_mode,
        );
        joined.truncate(1023);
        crate::paths::normalize_path_separators(&mut joined);
        dir_bytes = joined;
        let joined_cstr = cstring(dir_bytes.clone());
        host.set_option(option_name, &joined_cstr);
    }

    let dir_cstr = cstring(dir_bytes);
    if create {
        if let Err(errmsg) = host.ensure_directory(&dir_cstr) {
            let msg = format_bytes_msg(&[
                b"Invalid value for option \"",
                option_name.to_bytes(),
                b"\" (",
                dir_cstr.to_bytes(),
                b"): ",
                errmsg.to_bytes(),
            ]);
            host.config_error(&msg);
            return Some(dir_cstr);
        }
    }

    Some(dir_cstr)
}

/// Parses all scalar options in `Options::InitOptions` in exact historical order.
pub fn init_scalar_options_host<H: OptionsHost>(
    host: &mut H,
    out_memory_pressure_path: &mut Option<CString>,
    out_hard_linking_ignore_ext: &mut Option<CString>,
    out_dupe_search_url: &mut Option<CString>,
    out_dupe_search_api_key: &mut Option<CString>,
    out_warn_dupe_search_no_url: &mut bool,
) -> NzbgetRsParsedOptions {
    let mut s = NzbgetRsParsedOptions::default();

    s.download_rate = host.parse_int_value(c"DownloadRate", 10).wrapping_mul(1024);
    s.article_timeout = host.parse_int_value(c"ArticleTimeout", 10);
    s.article_read_chunk_size = host
        .parse_int_value(c"ArticleReadChunkSize", 10)
        .wrapping_mul(1024);
    s.url_timeout = host.parse_int_value(c"UrlTimeout", 10);
    s.remote_timeout = host.parse_int_value(c"RemoteTimeout", 10);
    s.article_retries = host.parse_int_value(c"ArticleRetries", 10);
    s.article_interval = host.parse_int_value(c"ArticleInterval", 10);
    s.url_retries = host.parse_int_value(c"UrlRetries", 10);
    s.url_interval = host.parse_int_value(c"UrlInterval", 10);
    s.control_port = host.parse_int_value(c"ControlPort", 10);
    s.secure_port = host.parse_int_value(c"SecurePort", 10);
    s.url_connections = host.parse_int_value(c"UrlConnections", 10);
    s.log_buffer = host.parse_int_value(c"LogBuffer", 10);
    s.rotate_log = host.parse_int_value(c"RotateLog", 10);
    s.umask = host.parse_int_value(c"UMask", 8);
    s.update_interval = host.parse_int_value(c"UpdateInterval", 10);
    s.write_buffer = host.parse_int_value(c"WriteBuffer", 10);
    s.nzb_dir_interval = host.parse_int_value(c"NzbDirInterval", 10);
    s.nzb_dir_file_age = host.parse_int_value(c"NzbDirFileAge", 10);
    s.nzb_dir_archive_scan = i32::from(host.parse_bool_value(c"NzbDirArchiveScan"));
    s.nzb_dir_archive_action = host.parse_enum_value(
        c"NzbDirArchiveAction",
        &ARCHIVE_ACTION_NAMES,
        &ARCHIVE_ACTION_VALUES,
    );
    s.disk_space = host.parse_int_value(c"DiskSpace", 10);
    s.par_time_limit = host.parse_int_value(c"ParTimeLimit", 10);
    s.keep_history = host.parse_int_value(c"KeepHistory", 10);
    s.feed_history = host.parse_int_value(c"FeedHistory", 10);
    s.time_correction = normalize_time_correction(host.parse_int_value(c"TimeCorrection", 10));
    s.propagation_delay =
        options_minutes_to_seconds_i32(host.parse_int_value(c"PropagationDelay", 10));
    s.article_cache = host.parse_int_value(c"ArticleCache", 10);
    *out_memory_pressure_path = host.get_option(c"MemoryPressurePath");
    let raw_mem_pressure_interval = host.parse_int_value(c"MemoryPressureInterval", 10);
    s.memory_pressure_interval = if raw_mem_pressure_interval > 0 {
        raw_mem_pressure_interval
    } else {
        500
    };
    s.event_interval = host.parse_int_value(c"EventInterval", 10);
    s.par_buffer = host.parse_int_value(c"ParBuffer", 10);
    s.par_threads = host.parse_int_value(c"ParThreads", 10);
    s.monthly_quota = host.parse_int_value(c"MonthlyQuota", 10);
    s.quota_start_day = host.parse_int_value(c"QuotaStartDay", 10);
    s.daily_quota = host.parse_int_value(c"DailyQuota", 10);

    s.system_health_check = i32::from(host.parse_bool_value(c"SystemHealthCheck"));
    s.nzb_log = i32::from(host.parse_bool_value(c"NzbLog"));
    s.append_category_dir = i32::from(host.parse_bool_value(c"AppendCategoryDir"));
    s.continue_partial = i32::from(host.parse_bool_value(c"ContinuePartial"));
    s.flush_queue = i32::from(host.parse_bool_value(c"FlushQueue"));
    s.dupe_check = i32::from(host.parse_bool_value(c"DupeCheck"));
    s.par_repair = i32::from(host.parse_bool_value(c"ParRepair"));
    s.par_quick = i32::from(host.parse_bool_value(c"ParQuick"));
    s.par_rename = i32::from(host.parse_bool_value(c"ParRename"));
    s.rar_rename = i32::from(host.parse_bool_value(c"RarRename"));
    s.direct_rename = i32::from(host.parse_bool_value(c"DirectRename"));
    s.hard_linking = i32::from(host.parse_bool_value(c"HardLinking"));
    *out_hard_linking_ignore_ext = host.get_option(c"HardLinkingIgnoreExt");
    s.curses_nzb_name = i32::from(host.parse_bool_value(c"CursesNzbName"));
    s.curses_time = i32::from(host.parse_bool_value(c"CursesTime"));
    s.curses_group = i32::from(host.parse_bool_value(c"CursesGroup"));
    s.crc_check = i32::from(host.parse_bool_value(c"CrcCheck"));
    s.direct_write = i32::from(host.parse_bool_value(c"DirectWrite"));
    s.raw_article = i32::from(host.parse_bool_value(c"RawArticle"));
    s.skip_write = i32::from(host.parse_bool_value(c"SkipWrite"));
    s.crash_trace = i32::from(host.parse_bool_value(c"CrashTrace"));
    s.crash_dump = i32::from(host.parse_bool_value(c"CrashDump"));
    s.par_pause_queue = i32::from(host.parse_bool_value(c"ParPauseQueue"));
    s.script_pause_queue = i32::from(host.parse_bool_value(c"ScriptPauseQueue"));
    s.nzb_cleanup_disk = i32::from(host.parse_bool_value(c"NzbCleanupDisk"));
    s.form_auth = i32::from(host.parse_bool_value(c"FormAuth"));
    s.json_p = i32::from(host.parse_bool_value(c"JsonP"));
    s.secure_control = i32::from(host.parse_bool_value(c"SecureControl"));
    s.unpack = i32::from(host.parse_bool_value(c"Unpack"));
    s.direct_unpack = i32::from(host.parse_bool_value(c"DirectUnpack"));
    s.use_temp_unpack_dir = i32::from(host.parse_bool_value(c"UseTempUnpackDir"));
    s.unpack_cleanup_disk = i32::from(host.parse_bool_value(c"UnpackCleanupDisk"));
    s.unpack_pause_queue = i32::from(host.parse_bool_value(c"UnpackPauseQueue"));
    s.url_force = i32::from(host.parse_bool_value(c"UrlForce"));
    s.cert_check = i32::from(host.parse_bool_value(c"CertCheck"));
    s.reorder_files = i32::from(host.parse_bool_value(c"ReorderFiles"));
    s.post_download_rename = i32::from(host.parse_bool_value(c"PostDownloadRename"));
    s.rename_after_unpack = i32::from(host.parse_bool_value(c"RenameAfterUnpack"));

    s.output_mode = host.parse_enum_value(c"OutputMode", &OUTPUT_MODE_NAMES, &OUTPUT_MODE_VALUES);
    s.par_check = host.parse_enum_value(c"ParCheck", &PAR_CHECK_NAMES, &PAR_CHECK_VALUES);
    s.par_scan = host.parse_enum_value(c"ParScan", &PAR_SCAN_NAMES, &PAR_SCAN_VALUES);
    s.dupe_article_fallback = host.parse_enum_value(
        c"DupeArticleFallback",
        &DUPE_ARTICLE_FALLBACK_NAMES,
        &DUPE_ARTICLE_FALLBACK_VALUES,
    );
    s.dupe_stream_decompress = i32::from(host.parse_bool_value(c"DupeStreamDecompress"));
    let raw_stream_timeout = host.parse_int_value(c"DupeStreamTimeout", 10);
    s.dupe_search = i32::from(host.parse_bool_value(c"DupeSearch"));
    s.dupe_search_dry_run = i32::from(host.parse_bool_value(c"DupeSearchDryRun"));
    *out_dupe_search_url = host.get_option(c"DupeSearchUrl");
    *out_dupe_search_api_key = host.get_option(c"DupeSearchApiKey");
    let raw_search_delay = host.parse_int_value(c"DupeSearchDelay", 10);
    let raw_search_max_donors = host.parse_int_value(c"DupeSearchMaxDonors", 10);
    let raw_health_percent = host.parse_int_value(c"DupeHealthPercent", 10);
    let raw_health_min = host.parse_int_value(c"DupeHealthMin", 10);
    let raw_health_max = host.parse_int_value(c"DupeHealthMax", 10);
    let raw_body_checks = host.parse_int_value(c"DupeBodyChecks", 10);
    let raw_health_budget = host.parse_int_value(c"DupeHealthBudget", 10);
    let raw_min_alive = host.parse_int_value(c"DupeMinAlive", 10);
    let raw_fast_donors = host.parse_int_value(c"DupeFastDonors", 10);

    let (
        norm_mem,
        norm_stream_timeout,
        norm_search_delay,
        norm_max_donors,
        norm_health_pct,
        norm_health_min,
        norm_health_max,
        norm_body_checks,
        norm_health_budget,
        norm_min_alive,
        norm_fast_donors,
    ) = options_normalize_dupe_and_mem_options(
        raw_mem_pressure_interval,
        raw_stream_timeout,
        raw_search_delay,
        raw_search_max_donors,
        raw_health_percent,
        raw_health_min,
        raw_health_max,
        raw_body_checks,
        raw_health_budget,
        raw_min_alive,
        raw_fast_donors,
    );
    s.memory_pressure_interval = norm_mem;
    s.dupe_stream_timeout = norm_stream_timeout;
    s.dupe_search_delay = norm_search_delay;
    s.dupe_search_max_donors = norm_max_donors;
    s.dupe_health_percent = norm_health_pct;
    s.dupe_health_min = norm_health_min;
    s.dupe_health_max = norm_health_max;
    s.dupe_body_checks = norm_body_checks;
    s.dupe_health_budget = norm_health_budget;
    s.dupe_min_alive = norm_min_alive;
    s.dupe_fast_donors = norm_fast_donors;
    (
        s.dupe_primary_score,
        s.dupe_size_tolerance,
        s.dupe_body_percent,
        s.dupe_check_concurrency,
        s.dupe_check_connections,
    ) = options_normalize_dupe_check_options(
        host.parse_int_value(c"DupePrimaryScore", 10),
        host.parse_int_value(c"DupeSizeTolerance", 10),
        host.parse_int_value(c"DupeBodyPercent", 10),
        host.parse_int_value(c"DupeCheckConcurrency", 10),
        host.parse_int_value(c"DupeCheckConnections", 10),
    );

    let url_empty = out_dupe_search_url
        .as_ref()
        .is_none_or(|u| u.to_bytes().is_empty());
    if s.dupe_search != 0 && url_empty {
        *out_warn_dupe_search_no_url = true;
        s.dupe_search = 0;
    } else {
        *out_warn_dupe_search_no_url = false;
    }

    s.post_strategy =
        host.parse_enum_value(c"PostStrategy", &POST_STRATEGY_NAMES, &POST_STRATEGY_VALUES);
    s.file_naming = host.parse_enum_value(c"FileNaming", &FILE_NAMING_NAMES, &FILE_NAMING_VALUES);
    s.health_check =
        host.parse_enum_value(c"HealthCheck", &HEALTH_CHECK_NAMES, &HEALTH_CHECK_VALUES);
    s.info_target = host.parse_enum_value(c"InfoTarget", &TARGET_NAMES, &TARGET_VALUES);
    s.warning_target = host.parse_enum_value(c"WarningTarget", &TARGET_NAMES, &TARGET_VALUES);
    s.error_target = host.parse_enum_value(c"ErrorTarget", &TARGET_NAMES, &TARGET_VALUES);
    s.debug_target = host.parse_enum_value(c"DebugTarget", &TARGET_NAMES, &TARGET_VALUES);
    s.detail_target = host.parse_enum_value(c"DetailTarget", &TARGET_NAMES, &TARGET_VALUES);
    s.write_log = host.parse_enum_value(c"WriteLog", &WRITE_LOG_NAMES, &WRITE_LOG_VALUES);

    s
}

/// Implements `Options::CheckOptions` on top of `OptionsHost`.
#[allow(clippy::too_many_arguments)]
pub fn check_options_host<H: OptionsHost>(
    host: &mut H,
    scalars: &mut NzbgetRsParsedOptions,
    no_disk_access: bool,
    is_32bit: bool,
    disable_parcheck: bool,
    disable_curses: bool,
    disable_tls: bool,
    have_x509_check_host: bool,
    cert_store_empty: bool,
    web_dir: &[u8],
    config_template: &mut Option<CString>,
    unpack_pass_file: Option<&CStr>,
) {
    if disable_parcheck {
        if scalars.par_check != 3 {
            host.locate_option_src_pos(c"ParCheck");
            host.config_error(
                c"Invalid value for option \"ParCheck\": program was compiled without parcheck-support",
            );
        }
        if scalars.par_rename != 0 {
            host.locate_option_src_pos(c"ParRename");
            host.config_error(
                c"Invalid value for option \"ParRename\": program was compiled without parcheck-support",
            );
        }
        if scalars.direct_rename != 0 {
            host.locate_option_src_pos(c"DirectRename");
            host.config_error(
                c"Invalid value for option \"DirectRename\": program was compiled without parcheck-support",
            );
        }
    }

    if disable_curses && scalars.output_mode == 2 {
        host.locate_option_src_pos(c"OutputMode");
        host.config_error(
            c"Invalid value for option \"OutputMode\": program was compiled without curses-support",
        );
    }

    if disable_tls {
        if scalars.secure_control != 0 {
            host.locate_option_src_pos(c"SecureControl");
            host.config_error(
                c"Invalid value for option \"SecureControl\": program was compiled without TLS/SSL-support",
            );
        }
        if scalars.cert_check != 0 {
            host.locate_option_src_pos(c"CertCheck");
            host.config_error(
                c"Invalid value for option \"CertCheck\": program was compiled without TLS/SSL-support",
            );
        }
    } else if !have_x509_check_host && scalars.cert_check != 0 {
        host.locate_option_src_pos(c"CertCheck");
        host.config_warn(
            c"TLS certificate verification (option \"CertCheck\") is limited because the program was compiled with older OpenSSL version not supporting hostname validation (at least OpenSSL 1.0.2d is required)",
        );
    }

    if scalars.cert_check != 0 && cert_store_empty {
        host.locate_option_src_pos(c"CertCheck");
        host.config_error(
            c"Option \"CertCheck\" requires proper configuration of option \"CertStore\"",
        );
        scalars.cert_check = 0;
    }

    let (dw, dr) = options_reconcile_write_flags(
        scalars.raw_article != 0,
        scalars.skip_write != 0,
        scalars.direct_write != 0,
        scalars.direct_rename != 0,
    );
    scalars.direct_write = i32::from(dw);
    scalars.direct_rename = i32::from(dr);

    let ct_empty = config_template
        .as_ref()
        .is_none_or(|c| c.to_bytes().is_empty());
    if ct_empty && !no_disk_access && !web_dir.is_empty() {
        let candidate = options_format_default_conf_path(web_dir, crate::paths::PATH_SEPARATOR);
        let candidate_cstr = cstring(candidate);
        if host.file_exists(&candidate_cstr) {
            *config_template = Some(candidate_cstr);
        } else {
            *config_template = Some(c"".to_owned());
        }
    }

    let orig_ac = scalars.article_cache;
    let orig_pb = scalars.par_buffer;
    let (new_ac, new_pb, mem_err) = clamp_32bit_memory_limits(is_32bit, orig_ac, orig_pb);
    scalars.article_cache = new_ac;
    scalars.par_buffer = new_pb;
    if mem_err == 1 {
        let msg = cstring(
            format!(
                "Invalid value for option \"ArticleCache\": {}. Changed to 1900",
                orig_ac
            )
            .into_bytes(),
        );
        host.config_error(&msg);
    } else if mem_err == 2 {
        let msg = cstring(
            format!(
                "Invalid value for option \"ParBuffer\": {}. Changed to 1900",
                orig_pb
            )
            .into_bytes(),
        );
        host.config_error(&msg);
    } else if mem_err == 3 {
        if orig_ac > 1900 {
            let msg = cstring(
                format!(
                    "Invalid value for option \"ArticleCache\": {}. Changed to 1900",
                    orig_ac
                )
                .into_bytes(),
            );
            host.config_error(&msg);
        } else if orig_pb > 1900 {
            let msg = cstring(
                format!(
                    "Invalid value for option \"ParBuffer\": {}. Changed to 1900",
                    orig_pb
                )
                .into_bytes(),
            );
            host.config_error(&msg);
        }
        host.config_error(
            c"Options \"ArticleCache\" and \"ParBuffer\" in total cannot use more than 1900MB of memory in 32-Bit mode. Changed to 1500 and 400",
        );
    }

    if let Some(upf) = unpack_pass_file {
        if !upf.to_bytes().is_empty() && !host.file_exists(upf) {
            let msg = format_bytes_msg(&[
                b"Invalid value for option \"UnpackPassFile\": ",
                upf.to_bytes(),
                b". File not found",
            ]);
            host.config_error(&msg);
        }
    }

    let mut n = 1;
    loop {
        let enc_opt = cstring(options_format_indexed_opt_name(b"Server", n, b"Encryption"));
        let cipher_opt = cstring(options_format_indexed_opt_name(b"Server", n, b"Cipher"));
        let ntls = host.get_option(&enc_opt);
        let ncipher = host.get_option(&cipher_opt);
        if ntls.is_none()
            && ncipher.is_none()
            && host
                .get_option(&cstring(options_format_indexed_opt_name(
                    b"Server", n, b"Host",
                )))
                .is_none()
            && host
                .get_option(&cstring(options_format_indexed_opt_name(
                    b"Server", n, b"Name",
                )))
                .is_none()
        {
            break;
        }
        let tls = if ntls.is_some() {
            host.parse_bool_value(&enc_opt)
        } else {
            false
        };
        if !tls && ncipher.as_ref().is_some_and(|c| !c.to_bytes().is_empty()) {
            let cipher_val = ncipher.unwrap();
            host.locate_option_src_pos(&cipher_opt);
            let n_str = n.to_string();
            let msg = format_bytes_msg(&[
                b"Option ",
                cipher_opt.to_bytes(),
                b" ('",
                cipher_val.to_bytes(),
                b"') is ignored because ",
                enc_opt.to_bytes(),
                b" is disabled for Server",
                n_str.as_bytes(),
                b".",
            ]);
            host.config_warn(&msg);
            host.set_option(&cipher_opt, c"");
        }
        n += 1;
    }

    let mut disable_dupe = 0;
    let mut enable_flush = 0;
    let mut enforce_mutex = 0;
    unsafe {
        roc_options_reconcile_conflict_rules(
            scalars.dupe_search,
            scalars.health_check,
            2, // hcPark
            scalars.flush_queue,
            scalars.direct_write,
            scalars.unpack_pause_queue,
            scalars.direct_unpack,
            &mut disable_dupe,
            &mut enable_flush,
            &mut enforce_mutex,
        );
    }

    if disable_dupe != 0 {
        host.locate_option_src_pos(c"DupeSearch");
        host.config_warn(
            c"Option 'DupeSearch' automatically disabled: 'HealthCheck' is set to 'park', which suspends failed downloads without triggering duplicate donor resolution.",
        );
        scalars.dupe_search = 0;
        host.set_option(c"DupeSearch", c"no");
    }

    if enable_flush != 0 {
        host.locate_option_src_pos(c"FlushQueue");
        host.config_warn(
            c"Option 'FlushQueue=no' is incompatible with 'DirectWrite=yes'. Automatically enabling 'FlushQueue=yes' to ensure queue crash-recovery and prevent duplicate file corruption.",
        );
        scalars.flush_queue = 1;
        host.set_option(c"FlushQueue", c"yes");
    }

    if enforce_mutex != 0 {
        host.locate_option_src_pos(c"DirectUnpack");
        host.config_warn(
            c"Option 'DirectUnpack=yes' with 'UnpackPauseQueue=no' can cause severe disk thrashing. Unpack concurrency will be serialized across jobs to protect I/O throughput.",
        );
        scalars.unpack_mutex_enforced = 1;
        host.set_option(c"_UnpackMutexEnforced", c"yes");
    }
}

fn c_atoi_or_default(val: Option<&CStr>, default_val: i32) -> i32 {
    match val {
        Some(s) => crate::feed::c_atoi_bytes(s.to_bytes()),
        None => default_val,
    }
}

/// Implements `Options::InitServers` on top of `OptionsHost`.
pub fn init_servers_host<H: OptionsHost>(
    host: &mut H,
    disable_tls: bool,
    scalars: &mut NzbgetRsParsedOptions,
) {
    let mut n = 1;
    loop {
        let active_opt = cstring(options_format_indexed_opt_name(b"Server", n, b"Active"));
        let nactive = host.get_option(&active_opt);
        let active = if nactive.is_some() {
            host.parse_bool_value(&active_opt)
        } else {
            true
        };

        let nname = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server", n, b"Name",
        )));
        let nlevel = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server", n, b"Level",
        )));
        let ngroup = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server", n, b"Group",
        )));
        let nhost = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server", n, b"Host",
        )));
        let nport = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server", n, b"Port",
        )));
        let nusername = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server",
            n,
            b"Username",
        )));
        let npassword = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server",
            n,
            b"Password",
        )));

        let join_group_opt = cstring(options_format_indexed_opt_name(b"Server", n, b"JoinGroup"));
        let njoingroup = host.get_option(&join_group_opt);
        let join_group = if njoingroup.is_some() {
            host.parse_bool_value(&join_group_opt)
        } else {
            false
        };

        let optional_opt = cstring(options_format_indexed_opt_name(b"Server", n, b"Optional"));
        let noptional = host.get_option(&optional_opt);
        let optional = if noptional.is_some() {
            host.parse_bool_value(&optional_opt)
        } else {
            false
        };

        let tls_opt = cstring(options_format_indexed_opt_name(b"Server", n, b"Encryption"));
        let ntls = host.get_option(&tls_opt);
        let mut tls = false;
        if ntls.is_some() {
            tls = host.parse_bool_value(&tls_opt);
            if disable_tls && tls {
                let msg = format_bytes_msg(&[
                    b"Invalid value for option \"",
                    tls_opt.to_bytes(),
                    b"\": program was compiled without TLS/SSL-support",
                ]);
                host.config_error(&msg);
                tls = false;
            }
            if tls {
                scalars.tls = 1;
            }
        }

        let cert_verif_opt = cstring(options_format_indexed_opt_name(
            b"Server",
            n,
            b"CertVerification",
        ));
        let ncertverif = host.get_option(&cert_verif_opt);
        let cert_verif_level = if ncertverif.is_some() {
            host.parse_enum_value(&cert_verif_opt, &CERT_VERIF_NAMES, &CERT_VERIF_VALUES)
        } else {
            2 // cvStrict
        };

        let ip_ver_opt = cstring(options_format_indexed_opt_name(b"Server", n, b"IpVersion"));
        let nipver = host.get_option(&ip_ver_opt);
        let ip_version = if nipver.is_some() {
            host.parse_enum_value(&ip_ver_opt, &IP_VERSION_NAMES, &IP_VERSION_VALUES)
        } else {
            0
        };

        let ncipher = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server", n, b"Cipher",
        )));
        let nconnections = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server",
            n,
            b"Connections",
        )));
        let nretention = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Server",
            n,
            b"Retention",
        )));

        let definition = nactive.is_some()
            || nname.is_some()
            || nlevel.is_some()
            || ngroup.is_some()
            || nhost.is_some()
            || nport.is_some()
            || noptional.is_some()
            || nusername.is_some()
            || npassword.is_some()
            || nconnections.is_some()
            || njoingroup.is_some()
            || ntls.is_some()
            || ncipher.is_some()
            || nretention.is_some();

        let state = options_check_server_def(
            definition,
            nhost.is_some(),
            nport.is_some(),
            nconnections.is_some(),
        );
        if state == 0 {
            break;
        }
        if state == 1 {
            host.add_news_server(
                n,
                active,
                nname.as_deref(),
                nhost.as_deref(),
                c_atoi_or_default(nport.as_deref(), 119),
                ip_version,
                nusername.as_deref(),
                npassword.as_deref(),
                join_group,
                tls,
                ncipher.as_deref(),
                c_atoi_or_default(nconnections.as_deref(), 1),
                c_atoi_or_default(nretention.as_deref(), 0),
                c_atoi_or_default(nlevel.as_deref(), 0),
                c_atoi_or_default(ngroup.as_deref(), 0),
                optional,
                cert_verif_level as u32,
            );
        } else {
            let msg =
                cstring(format!("Server definition not complete for \"Server{}\"", n).into_bytes());
            host.config_error(&msg);
        }

        n += 1;
    }
}

/// Implements `Options::InitCategories` on top of `OptionsHost`.
pub fn init_categories_host<H: OptionsHost>(
    host: &mut H,
    no_disk_access: bool,
    dest_dir: Option<&CStr>,
) {
    let mut n = 1;
    loop {
        let nname = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Category",
            n,
            b"Name",
        )));
        let dest_dir_opt = cstring(options_format_indexed_opt_name(b"Category", n, b"DestDir"));
        let ndestdir = host.get_option(&dest_dir_opt);

        let unpack_opt = cstring(options_format_indexed_opt_name(b"Category", n, b"Unpack"));
        let nunpack = host.get_option(&unpack_opt);
        let unpack = if nunpack.is_some() {
            host.parse_bool_value(&unpack_opt)
        } else {
            true
        };

        let naliases = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Category",
            n,
            b"Aliases",
        )));

        let definition =
            nname.is_some() || ndestdir.is_some() || nunpack.is_some() || naliases.is_some();
        let has_name = nname.as_ref().is_some_and(|s| !s.to_bytes().is_empty());
        let state = options_check_category_or_feed_def(definition, has_name);
        if state == 0 {
            break;
        }
        if state == 1 {
            let resolved_dest_dir = if ndestdir.as_ref().is_some_and(|s| !s.to_bytes().is_empty()) {
                check_dir_host(host, no_disk_access, &dest_dir_opt, dest_dir, false, false)
            } else {
                None
            };
            let aliases: Vec<CString> = match naliases.as_ref() {
                Some(a) => crate::util::tokens(a.to_bytes(), b",;")
                    .map(|t| cstring(t.to_vec()))
                    .collect(),
                None => Vec::new(),
            };
            host.add_category(
                nname.as_deref().unwrap_or(c""),
                resolved_dest_dir.as_deref(),
                unpack,
                &aliases,
            );
        } else {
            let msg = cstring(
                format!("Category definition not complete for \"Category{}\"", n).into_bytes(),
            );
            host.config_error(&msg);
        }

        n += 1;
    }
}

/// Implements `Options::InitFeeds` on top of `OptionsHost`.
pub fn init_feeds_host<H: OptionsHost>(host: &mut H) {
    let mut n = 1;
    loop {
        let nname = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Feed", n, b"Name",
        )));
        let nurl = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Feed", n, b"URL",
        )));
        let nfilter = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Feed", n, b"Filter",
        )));
        let ncategory = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Feed",
            n,
            b"Category",
        )));

        let backlog_opt = cstring(options_format_indexed_opt_name(b"Feed", n, b"Backlog"));
        let nbacklog = host.get_option(&backlog_opt);
        let backlog = if nbacklog.is_some() {
            host.parse_bool_value(&backlog_opt)
        } else {
            true
        };

        let pause_nzb_opt = cstring(options_format_indexed_opt_name(b"Feed", n, b"PauseNzb"));
        let npausenzb = host.get_option(&pause_nzb_opt);
        let pause_nzb = if npausenzb.is_some() {
            host.parse_bool_value(&pause_nzb_opt)
        } else {
            false
        };

        let ncategory_source = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Feed",
            n,
            b"CategorySource",
        )));
        let category_source = parse_category_source(ncategory_source.as_deref());

        let ninterval = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Feed",
            n,
            b"Interval",
        )));
        let npriority = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Feed",
            n,
            b"Priority",
        )));

        let cert_verif_opt = cstring(options_format_indexed_opt_name(
            b"Feed",
            n,
            b"CertVerification",
        ));
        let ncertverif = host.get_option(&cert_verif_opt);
        let cert_verif_level = if ncertverif.is_some() {
            host.parse_enum_value(&cert_verif_opt, &CERT_VERIF_NAMES, &CERT_VERIF_VALUES)
        } else {
            2 // cvStrict
        };

        let definition = nname.is_some()
            || nurl.is_some()
            || nfilter.is_some()
            || ncategory.is_some()
            || nbacklog.is_some()
            || npausenzb.is_some()
            || ninterval.is_some()
            || npriority.is_some();
        let state = options_check_category_or_feed_def(definition, nurl.is_some());
        if state == 0 {
            break;
        }
        if state == 1 {
            host.add_feed(
                n,
                nname.as_deref(),
                nurl.as_deref(),
                c_atoi_or_default(ninterval.as_deref(), 0),
                nfilter.as_deref(),
                backlog,
                pause_nzb,
                ncategory.as_deref(),
                category_source,
                c_atoi_or_default(npriority.as_deref(), 0),
                cert_verif_level as u32,
            );
        } else {
            let msg =
                cstring(format!("Feed definition not complete for \"Feed{}\"", n).into_bytes());
            host.config_error(&msg);
        }

        n += 1;
    }
}

/// Implements `Options::CreateSchedulerTask` on top of `OptionsHost`.
pub fn create_scheduler_task_host<H: OptionsHost>(
    host: &mut H,
    id: i32,
    time: Option<&CStr>,
    week_days: Option<&CStr>,
    command: i32,
    param: Option<&CStr>,
) {
    if id == 0 {
        host.set_config_line(0);
    }

    let mut week_days_val = 0;
    if let Some(wd) = week_days {
        if !parse_week_days(wd.to_bytes(), &mut week_days_val) {
            let id_str = id.to_string();
            let msg = format_bytes_msg(&[
                b"Invalid value for option \"Task",
                id_str.as_bytes(),
                b".WeekDays\": \"",
                wd.to_bytes(),
                b"\"",
            ]);
            host.config_error(&msg);
            return;
        }
    }

    let Some(time_cstr) = time else {
        return;
    };
    for one_time in crate::util::tokens(time_cstr.to_bytes(), b";,") {
        let one_time_cstr = cstring(one_time.to_vec());
        let (mut hours, mut minutes) = (0, 0);
        if !parse_time(&one_time_cstr, &mut hours, &mut minutes) {
            let id_str = id.to_string();
            let msg = format_bytes_msg(&[
                b"Invalid value for option \"Task",
                id_str.as_bytes(),
                b".Time\": \"",
                one_time_cstr.to_bytes(),
                b"\"",
            ]);
            host.config_error(&msg);
            return;
        }

        if options_is_every_hour_task(hours) {
            for every_hour in 0..24 {
                host.add_task(id, every_hour, minutes, week_days_val, command, param);
            }
        } else {
            host.add_task(id, hours, minutes, week_days_val, command, param);
        }
    }
}

/// Implements `Options::InitScheduler` on top of `OptionsHost`.
pub fn init_scheduler_host<H: OptionsHost>(host: &mut H) {
    let mut n = 1;
    loop {
        let time = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Task", n, b"Time",
        )));
        let week_days = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Task",
            n,
            b"WeekDays",
        )));
        let cmd_opt = cstring(options_format_indexed_opt_name(b"Task", n, b"Command"));
        let command = host.get_option(&cmd_opt);
        let download_rate = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Task",
            n,
            b"DownloadRate",
        )));
        let process = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Task", n, b"Process",
        )));
        let param = host.get_option(&cstring(options_format_indexed_opt_name(
            b"Task", n, b"Param",
        )));

        let (param_source, definition, completed) = options_resolve_scheduler_task_state(
            time.is_some(),
            week_days.is_some(),
            command.is_some(),
            download_rate.is_some(),
            param.is_some(),
            param.as_ref().is_none_or(|s| s.to_bytes().is_empty()),
            process.as_ref().is_none_or(|s| s.to_bytes().is_empty()),
            download_rate
                .as_ref()
                .is_none_or(|s| s.to_bytes().is_empty()),
        );
        let effective_param = match param_source {
            1 => process.as_deref(),
            2 => download_rate.as_deref(),
            _ => param.as_deref(),
        };

        if !definition {
            break;
        }
        if !completed {
            let msg =
                cstring(format!("Task definition not complete for \"Task{}\"", n).into_bytes());
            host.config_error(&msg);
            n += 1;
            continue;
        }

        let known_command = command
            .as_ref()
            .is_some_and(|c| options_lookup_scheduler_command(c.to_bytes()) >= 0);
        let task_command = host.parse_enum_value(&cmd_opt, &COMMAND_NAMES, &COMMAND_VALUES);
        if !known_command {
            n += 1;
            continue;
        }

        let split_cmd_empty = match effective_param {
            Some(p) if !p.to_bytes().is_empty() && task_command == 6 => {
                crate::util::split_command_line(p.to_bytes()).is_empty()
            }
            _ => false,
        };
        let param_check = options_validate_scheduler_param(
            task_command,
            effective_param.map(|p| p.to_bytes()),
            split_cmd_empty,
        );
        if param_check == 1 {
            let msg = cstring(format!("Invalid value for option \"Task{}.Param\"", n).into_bytes());
            host.config_error(&msg);
            n += 1;
            continue;
        }
        if param_check == 2 {
            let n_str = n.to_string();
            let bad_bytes = download_rate
                .as_deref()
                .or(effective_param)
                .map_or(&b""[..], CStr::to_bytes);
            let msg = format_bytes_msg(&[
                b"Invalid value for option \"Task",
                n_str.as_bytes(),
                b".Param\": \"",
                bad_bytes,
                b"\"",
            ]);
            host.config_error(&msg);
            n += 1;
            continue;
        }
        if param_check == 3 {
            let msg = cstring(
                format!(
                    "Task definition not complete for \"Task{}\". Option \"Task{}.Param\" is missing",
                    n, n
                )
                .into_bytes(),
            );
            host.config_error(&msg);
            n += 1;
            continue;
        }

        create_scheduler_task_host(
            host,
            n,
            time.as_deref(),
            week_days.as_deref(),
            task_command,
            effective_param,
        );
        n += 1;
    }
}

/// Pure-Rust option entry (`Options::OptEntry`).
#[derive(Clone, Debug)]
pub struct RustOptEntry {
    pub name: CString,
    pub value: Option<CString>,
    pub def_value: Option<CString>,
    pub line_no: i32,
}

impl RustOptEntry {
    pub fn new(name: &CStr, value: Option<&CStr>) -> Self {
        let mut entry = Self {
            name: name.to_owned(),
            value: None,
            def_value: None,
            line_no: 0,
        };
        if let Some(v) = value {
            entry.set_value(v);
        }
        entry
    }

    pub fn set_value(&mut self, value: &CStr) {
        self.value = Some(value.to_owned());
        if self.def_value.is_none() {
            self.def_value = Some(value.to_owned());
        }
    }

    pub fn restricted(&self) -> bool {
        option_restricted(&self.name, c_char::MIN < 0)
    }
}

/// Pure-Rust category definition (`Options::Category`).
#[derive(Clone, Debug)]
pub struct RustCategory {
    pub name: CString,
    pub dest_dir: CString,
    pub unpack: bool,
    pub aliases: Vec<CString>,
}

/// Pure-Rust extender interface (`Options::Extender`).
pub trait RustOptionsExtender {
    #[allow(clippy::too_many_arguments)]
    fn add_news_server(
        &mut self,
        id: i32,
        active: bool,
        name: Option<&CStr>,
        host: Option<&CStr>,
        port: i32,
        ip_version: i32,
        user: Option<&CStr>,
        pass: Option<&CStr>,
        join_group: bool,
        tls: bool,
        cipher: Option<&CStr>,
        max_connections: i32,
        retention: i32,
        level: i32,
        group: i32,
        optional: bool,
        cert_verif_level: u32,
    );
    #[allow(clippy::too_many_arguments)]
    fn add_feed(
        &mut self,
        id: i32,
        name: Option<&CStr>,
        url: Option<&CStr>,
        interval: i32,
        filter: Option<&CStr>,
        backlog: bool,
        pause_nzb: bool,
        category: Option<&CStr>,
        category_source: i32,
        priority: i32,
        cert_verif_level: u32,
    );
    fn add_task(
        &mut self,
        id: i32,
        hours: i32,
        minutes: i32,
        week_days_bits: i32,
        command: i32,
        param: Option<&CStr>,
    );
    fn setup_first_start(&mut self) {}
}

/// Pure-Rust implementation of `Options` (`daemon/main/Options.h` & `daemon/main/Options.cpp`).
pub struct RustOptions<'a> {
    pub opt_entries: Vec<RustOptEntry>,
    pub categories: Vec<RustCategory>,
    pub no_disk_access: bool,
    pub no_config: bool,
    pub fatal_error: bool,
    pub config_errors: bool,
    pub config_line: i32,
    pub config_filename: Option<CString>,
    pub app_dir: CString,
    pub main_dir: CString,
    pub dest_dir: CString,
    pub inter_dir: CString,
    pub temp_dir: CString,
    pub queue_dir: CString,
    pub nzb_dir: CString,
    pub web_dir: CString,
    pub config_template: CString,
    pub required_dir: CString,
    pub control_ip: CString,
    pub control_username: CString,
    pub control_password: CString,
    pub restricted_username: CString,
    pub restricted_password: CString,
    pub add_username: CString,
    pub add_password: CString,
    pub secure_cert: CString,
    pub secure_key: CString,
    pub cert_store: CString,
    pub authorized_ip: CString,
    pub lock_file: CString,
    pub daemon_username: CString,
    pub log_file: CString,
    pub unrar_cmd: CString,
    pub seven_zip_cmd: CString,
    pub unrar_path: CString,
    pub seven_zip_path: CString,
    pub unpack_pass_file: CString,
    pub ext_cleanup_disk: CString,
    pub par_ignore_ext: CString,
    pub unpack_ignore_ext: CString,
    pub rename_ignore_ext: CString,
    pub hard_linking_ignore_ext: CString,
    pub memory_pressure_path: CString,
    pub shell_override: CString,
    pub dupe_search_url: CString,
    pub dupe_search_api_key: CString,
    pub scalars: NzbgetRsParsedOptions,
    extender: Option<&'a mut dyn RustOptionsExtender>,
}

impl<'a> RustOptions<'a> {
    pub fn new_no_config(
        cmd_opts: Option<&[&CStr]>,
        extender: Option<&'a mut dyn RustOptionsExtender>,
    ) -> Self {
        let mut opts = Self {
            opt_entries: Vec::new(),
            categories: Vec::new(),
            no_disk_access: true,
            no_config: true,
            fatal_error: false,
            config_errors: false,
            config_line: 0,
            config_filename: None,
            app_dir: c"".to_owned(),
            main_dir: c"".to_owned(),
            dest_dir: c"".to_owned(),
            inter_dir: c"".to_owned(),
            temp_dir: c"".to_owned(),
            queue_dir: c"".to_owned(),
            nzb_dir: c"".to_owned(),
            web_dir: c"".to_owned(),
            config_template: c"".to_owned(),
            required_dir: c"".to_owned(),
            control_ip: c"".to_owned(),
            control_username: c"".to_owned(),
            control_password: c"".to_owned(),
            restricted_username: c"".to_owned(),
            restricted_password: c"".to_owned(),
            add_username: c"".to_owned(),
            add_password: c"".to_owned(),
            secure_cert: c"".to_owned(),
            secure_key: c"".to_owned(),
            cert_store: c"".to_owned(),
            authorized_ip: c"".to_owned(),
            lock_file: c"".to_owned(),
            daemon_username: c"".to_owned(),
            log_file: c"".to_owned(),
            unrar_cmd: c"".to_owned(),
            seven_zip_cmd: c"".to_owned(),
            unrar_path: c"".to_owned(),
            seven_zip_path: c"".to_owned(),
            unpack_pass_file: c"".to_owned(),
            ext_cleanup_disk: c"".to_owned(),
            par_ignore_ext: c"".to_owned(),
            unpack_ignore_ext: c"".to_owned(),
            rename_ignore_ext: c"".to_owned(),
            hard_linking_ignore_ext: c"".to_owned(),
            memory_pressure_path: c"".to_owned(),
            shell_override: c"".to_owned(),
            dupe_search_url: c"".to_owned(),
            dupe_search_api_key: c"".to_owned(),
            scalars: NzbgetRsParsedOptions::with_defaults(),
            extender,
        };
        opts.init(c"nzbget/nzbget", None, true, cmd_opts, true);
        opts
    }

    fn init(
        &mut self,
        exe_name: &CStr,
        config_filename: Option<&CStr>,
        no_config: bool,
        cmd_opts: Option<&[&CStr]>,
        no_disk_access: bool,
    ) {
        self.no_disk_access = no_disk_access;
        self.no_config = no_config;
        self.config_filename = config_filename.map(|c| c.to_owned());

        self.set_option(c"ConfigFile", c"");
        let mut filename = exe_name.to_bytes().to_vec();
        crate::paths::normalize_path_separators(&mut filename);
        let filename_cstr = cstring(filename.clone());
        self.set_option(c"AppBin", &filename_cstr);
        let parent_dir =
            crate::util::fs_extract_parent_dir_path(&filename, crate::paths::PATH_SEPARATOR);
        let app_dir_cstr = cstring(parent_dir);
        self.set_option(c"AppDir", &app_dir_cstr);
        self.app_dir = app_dir_cstr;
        self.set_option(c"Version", c"");

        init_defaults_with(false, |name, val| self.set_option(name, val));

        if let Some(list) = cmd_opts {
            for &opt in list {
                self.set_option_string(opt);
            }
        }

        if self.config_filename.is_none() && !no_config {
            self.fatal_error = true;
            return;
        }

        for e in &mut self.opt_entries {
            if e.value.is_none() {
                e.value = Some(c"".to_owned());
            }
        }

        self.check_dirs();
        self.init_options();
        let mut ct = Some(self.config_template.clone());
        let web_dir = self.web_dir.to_bytes().to_vec();
        let upf = self.unpack_pass_file.clone();
        let cert_store_empty = self.cert_store.to_bytes().is_empty();
        let mut scalars = self.scalars;
        check_options_host(
            self,
            &mut scalars,
            self.no_disk_access,
            usize::BITS == 32,
            false,
            false,
            false,
            true,
            cert_store_empty,
            &web_dir,
            &mut ct,
            Some(&upf),
        );
        self.scalars = scalars;
        if let Some(new_ct) = ct {
            self.config_template = new_ct;
        }

        let mut scalars = self.scalars;
        init_servers_host(self, false, &mut scalars);
        self.scalars = scalars;
        let dest_dir = self.dest_dir.clone();
        let no_disk = self.no_disk_access;
        init_categories_host(self, no_disk, Some(&dest_dir));
        init_scheduler_host(self);
        init_feeds_host(self);
    }

    fn check_dirs(&mut self) {
        self.main_dir = self
            .get_option(c"MainDir")
            .unwrap_or_else(|| c"".to_owned());
        let main_dir = self.main_dir.clone();
        let no_disk = self.no_disk_access;
        self.dest_dir = check_dir_host(self, no_disk, c"DestDir", Some(&main_dir), false, true)
            .unwrap_or_else(|| c"".to_owned());
        self.inter_dir = check_dir_host(self, no_disk, c"InterDir", Some(&main_dir), true, true)
            .unwrap_or_else(|| c"".to_owned());
        self.temp_dir = check_dir_host(self, no_disk, c"TempDir", Some(&main_dir), false, true)
            .unwrap_or_else(|| c"".to_owned());
        self.queue_dir = check_dir_host(self, no_disk, c"QueueDir", Some(&main_dir), false, true)
            .unwrap_or_else(|| c"".to_owned());
        self.web_dir = check_dir_host(self, no_disk, c"WebDir", None, true, false)
            .unwrap_or_else(|| c"".to_owned());
        self.nzb_dir = check_dir_host(self, no_disk, c"NzbDir", Some(&main_dir), false, true)
            .unwrap_or_else(|| c"".to_owned());
    }

    fn init_options(&mut self) {
        self.required_dir = self
            .get_option(c"RequiredDir")
            .unwrap_or_else(|| c"".to_owned());
        self.config_template = self
            .get_option(c"ConfigTemplate")
            .unwrap_or_else(|| c"".to_owned());
        self.control_ip = self
            .get_option(c"ControlIP")
            .unwrap_or_else(|| c"".to_owned());
        self.control_username = self
            .get_option(c"ControlUsername")
            .unwrap_or_else(|| c"".to_owned());
        self.control_password = self
            .get_option(c"ControlPassword")
            .unwrap_or_else(|| c"".to_owned());
        self.restricted_username = self
            .get_option(c"RestrictedUsername")
            .unwrap_or_else(|| c"".to_owned());
        self.restricted_password = self
            .get_option(c"RestrictedPassword")
            .unwrap_or_else(|| c"".to_owned());
        self.add_username = self
            .get_option(c"AddUsername")
            .unwrap_or_else(|| c"".to_owned());
        self.add_password = self
            .get_option(c"AddPassword")
            .unwrap_or_else(|| c"".to_owned());
        self.secure_cert = self
            .get_option(c"SecureCert")
            .unwrap_or_else(|| c"".to_owned());
        self.secure_key = self
            .get_option(c"SecureKey")
            .unwrap_or_else(|| c"".to_owned());
        self.cert_store = self
            .get_option(c"CertStore")
            .unwrap_or_else(|| c"".to_owned());
        self.authorized_ip = self
            .get_option(c"AuthorizedIP")
            .unwrap_or_else(|| c"".to_owned());
        self.lock_file = self
            .get_option(c"LockFile")
            .unwrap_or_else(|| c"".to_owned());
        self.daemon_username = self
            .get_option(c"DaemonUsername")
            .unwrap_or_else(|| c"".to_owned());
        self.log_file = self
            .get_option(c"LogFile")
            .unwrap_or_else(|| c"".to_owned());
        self.unrar_cmd = self
            .get_option(c"UnrarCmd")
            .unwrap_or_else(|| c"".to_owned());
        self.seven_zip_cmd = self
            .get_option(c"SevenZipCmd")
            .unwrap_or_else(|| c"".to_owned());
        self.unpack_pass_file = self
            .get_option(c"UnpackPassFile")
            .unwrap_or_else(|| c"".to_owned());
        self.ext_cleanup_disk = self
            .get_option(c"ExtCleanupDisk")
            .unwrap_or_else(|| c"".to_owned());
        self.par_ignore_ext = self
            .get_option(c"ParIgnoreExt")
            .unwrap_or_else(|| c"".to_owned());
        self.unpack_ignore_ext = self
            .get_option(c"UnpackIgnoreExt")
            .unwrap_or_else(|| c"".to_owned());

        self.unrar_path = cstring(resolve_tool_path_option(self.unrar_cmd.to_bytes()));
        self.seven_zip_path = cstring(resolve_tool_path_option(self.seven_zip_cmd.to_bytes()));
        self.shell_override = self
            .get_option(c"ShellOverride")
            .unwrap_or_else(|| c"".to_owned());
        self.rename_ignore_ext = self
            .get_option(c"RenameIgnoreExt")
            .unwrap_or_else(|| c"".to_owned());

        let mut mem_path = None;
        let mut hl_ext = None;
        let mut dupe_url = None;
        let mut dupe_key = None;
        let mut warn_no_url = false;
        self.scalars = init_scalar_options_host(
            self,
            &mut mem_path,
            &mut hl_ext,
            &mut dupe_url,
            &mut dupe_key,
            &mut warn_no_url,
        );
        self.memory_pressure_path = mem_path.unwrap_or_else(|| c"".to_owned());
        self.hard_linking_ignore_ext = hl_ext.unwrap_or_else(|| c"".to_owned());
        self.dupe_search_url = dupe_url.unwrap_or_else(|| c"".to_owned());
        self.dupe_search_api_key = dupe_key.unwrap_or_else(|| c"".to_owned());
    }

    pub fn set_option_string(&mut self, option: &CStr) -> bool {
        let Some((optname, optvalue)) = split_and_convert_option_string(option) else {
            let msg = format_bytes_msg(&[b"Invalid option \"", option.to_bytes(), b"\""]);
            self.config_error(&msg);
            return false;
        };
        let check = validate_option_name(&optname, &mut || self.get_option(&optname).is_some());
        match check {
            NameCheck::Valid => {
                self.set_option(&optname, &optvalue);
                true
            }
            NameCheck::Obsolete => {
                let msg = format_bytes_msg(&[
                    b"Option \"",
                    optname.to_bytes(),
                    b"\" is obsolete, ignored",
                ]);
                self.config_warn(&msg);
                true
            }
            NameCheck::ObsoleteLog => {
                let msg = format_bytes_msg(&[
                    b"Option \"",
                    optname.to_bytes(),
                    b"\" is obsolete, ignored, use \"WriteLog\" instead",
                ]);
                self.config_warn(&msg);
                true
            }
            NameCheck::Invalid => {
                let msg = format_bytes_msg(&[b"Invalid option \"", optname.to_bytes(), b"\""]);
                self.config_error(&msg);
                false
            }
        }
    }
}

impl OptionsHost for RustOptions<'_> {
    fn find_entry(&mut self, name: &CStr) -> Option<(Option<CString>, Option<CString>, i32)> {
        let entry = self.opt_entries.iter_mut().find(|e| eq(&e.name, name))?;
        if entry.name.to_bytes() != name.to_bytes() {
            entry.name = name.to_owned();
        }
        Some((entry.value.clone(), entry.def_value.clone(), entry.line_no))
    }

    fn set_entry_raw_value(&mut self, name: &CStr, value: &CStr) {
        if let Some(entry) = self.opt_entries.iter_mut().find(|e| eq(&e.name, name)) {
            entry.set_value(value);
        }
    }

    fn set_option(&mut self, name: &CStr, value: &CStr) {
        let idx = match self.opt_entries.iter().position(|e| eq(&e.name, name)) {
            Some(i) => {
                if self.opt_entries[i].name.to_bytes() != name.to_bytes() {
                    self.opt_entries[i].name = name.to_owned();
                }
                i
            }
            None => {
                self.opt_entries.push(RustOptEntry::new(name, None));
                self.opt_entries.len() - 1
            }
        };
        let curvalue =
            if options_should_expand_home_value(Some(value.to_bytes()), self.no_disk_access) {
                let home = std::env::var_os("HOME");
                #[cfg(unix)]
                let home_bytes = home.as_deref().map(std::os::unix::ffi::OsStrExt::as_bytes);
                #[cfg(not(unix))]
                let home_bytes: Option<&[u8]> = None;
                crate::fsops::expand_home_path(Some(value), home_bytes)
            } else {
                value.to_bytes().to_vec()
            };
        self.opt_entries[idx].line_no = self.config_line;
        let expanded = expand_option_variables(&curvalue, |var_name| {
            self.get_option(var_name).map(|c| c.into_bytes())
        });
        let expanded_cstr = cstring(expanded);
        self.opt_entries[idx].set_value(&expanded_cstr);
    }

    fn set_config_line(&mut self, line: i32) {
        self.config_line = line;
    }

    fn get_config_line(&self) -> i32 {
        self.config_line
    }

    fn config_error(&mut self, _msg: &CStr) {
        self.config_errors = true;
    }

    fn config_warn(&mut self, _msg: &CStr) {}

    fn file_exists(&self, path: &CStr) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let p = std::path::Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
            p.exists() && p.is_file()
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            false
        }
    }

    fn ensure_directory(&mut self, dir: &CStr) -> Result<(), CString> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let p = std::path::Path::new(std::ffi::OsStr::from_bytes(dir.to_bytes()));
            std::fs::create_dir_all(p).map_err(|e| cstring(e.to_string().into_bytes()))
        }
        #[cfg(not(unix))]
        {
            let _ = dir;
            Ok(())
        }
    }

    fn add_news_server(
        &mut self,
        id: i32,
        active: bool,
        name: Option<&CStr>,
        host: Option<&CStr>,
        port: i32,
        ip_version: i32,
        user: Option<&CStr>,
        pass: Option<&CStr>,
        join_group: bool,
        tls: bool,
        cipher: Option<&CStr>,
        max_connections: i32,
        retention: i32,
        level: i32,
        group: i32,
        optional: bool,
        cert_verif_level: u32,
    ) {
        if let Some(ext) = self.extender.as_deref_mut() {
            ext.add_news_server(
                id,
                active,
                name,
                host,
                port,
                ip_version,
                user,
                pass,
                join_group,
                tls,
                cipher,
                max_connections,
                retention,
                level,
                group,
                optional,
                cert_verif_level,
            );
        }
    }

    fn add_feed(
        &mut self,
        id: i32,
        name: Option<&CStr>,
        url: Option<&CStr>,
        interval: i32,
        filter: Option<&CStr>,
        backlog: bool,
        pause_nzb: bool,
        category: Option<&CStr>,
        category_source: i32,
        priority: i32,
        cert_verif_level: u32,
    ) {
        if let Some(ext) = self.extender.as_deref_mut() {
            ext.add_feed(
                id,
                name,
                url,
                interval,
                filter,
                backlog,
                pause_nzb,
                category,
                category_source,
                priority,
                cert_verif_level,
            );
        }
    }

    fn add_task(
        &mut self,
        id: i32,
        hours: i32,
        minutes: i32,
        week_days_bits: i32,
        command: i32,
        param: Option<&CStr>,
    ) {
        if let Some(ext) = self.extender.as_deref_mut() {
            ext.add_task(id, hours, minutes, week_days_bits, command, param);
        }
    }

    fn add_category(
        &mut self,
        name: &CStr,
        dest_dir: Option<&CStr>,
        unpack: bool,
        aliases: &[CString],
    ) {
        self.categories.push(RustCategory {
            name: name.to_owned(),
            dest_dir: dest_dir.unwrap_or(c"").to_owned(),
            unpack,
            aliases: aliases.to_vec(),
        });
    }
}

pub type NzbgetRsOptionsHostFn = Option<
    unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        op: c_int,
        ints: *const c_int,
        strs: *const *const c_char,
        count: usize,
    ),
>;

pub const OPT_STR_COUNT: usize = 41;

/// Heap-backed daemon `Options` instance (`NzbgetRsOptions`) driving full initialization,
/// string storage, and runtime scheduler task creation.
pub struct NzbgetRsOptions {
    opt_entries: Vec<RustOptEntry>,
    categories: Vec<RustCategory>,
    strs: [Option<CString>; OPT_STR_COUNT],
    no_disk_access: bool,
    no_config: bool,
    daemon_subsystems: bool,
    default_diag_sink: bool,
    fatal_error: bool,
    config_errors: std::sync::atomic::AtomicBool,
    mode_flags: std::sync::atomic::AtomicU8,
    config_line: i32,
    scalars: NzbgetRsParsedOptions,
    out_scalars: *mut NzbgetRsParsedOptions,
    cb_ctx: *mut std::ffi::c_void,
    cb_fn: NzbgetRsOptionsHostFn,
}

unsafe impl Send for NzbgetRsOptions {}
unsafe impl Sync for NzbgetRsOptions {}

static ACTIVE_OPTIONS: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsOptions>>> =
    std::sync::RwLock::new(None);

pub(crate) fn active_options() -> Option<std::sync::Arc<NzbgetRsOptions>> {
    ACTIVE_OPTIONS
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

impl NzbgetRsOptions {
    pub(crate) fn scalars(&self) -> &NzbgetRsParsedOptions {
        &self.scalars
    }

    pub(crate) fn has_fatal_error(&self) -> bool {
        self.fatal_error
    }

    pub(crate) fn set_mode_flag(&self, idx: usize, val: bool) {
        if idx < 4 {
            let bit = 1u8 << idx;
            if val {
                self.mode_flags
                    .fetch_or(bit, std::sync::atomic::Ordering::SeqCst);
            } else {
                self.mode_flags
                    .fetch_and(!bit, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    pub(crate) fn server_mode(&self) -> bool {
        (self.mode_flags.load(std::sync::atomic::Ordering::Relaxed) & 1) != 0
    }

    pub(crate) fn daemon_mode(&self) -> bool {
        (self.mode_flags.load(std::sync::atomic::Ordering::Relaxed) & 2) != 0
    }

    pub(crate) fn remote_client_mode(&self) -> bool {
        (self.mode_flags.load(std::sync::atomic::Ordering::Relaxed) & 4) != 0
    }

    pub(crate) fn direct_client_mode(&self) -> bool {
        (self.mode_flags.load(std::sync::atomic::Ordering::Relaxed) & 8) != 0
    }

    pub(crate) fn for_each_option(&self, mut cb: impl FnMut(&CStr, &CStr, c_int)) {
        let signed_char = c_char::MIN < 0;
        for e in &self.opt_entries {
            let val = e.value.as_deref().unwrap_or(c"");
            let restr = c_int::from(option_restricted(&e.name, signed_char));
            cb(&e.name, val, restr);
        }
    }

    pub(crate) fn str_cstr(&self, id: usize) -> Option<&CStr> {
        self.strs.get(id).and_then(|s| s.as_deref())
    }

    pub(crate) fn str_bytes(&self, id: usize) -> &[u8] {
        self.str_cstr(id).map_or(&[], CStr::to_bytes)
    }

    pub(crate) fn categories(&self) -> &[RustCategory] {
        &self.categories
    }

    pub(crate) fn find_category(&self, name: &CStr, search_aliases: bool) -> Option<&RustCategory> {
        for cat in &self.categories {
            if eq(&cat.name, name) {
                return Some(cat);
            }
        }
        if search_aliases {
            let name_bytes = name.to_bytes();
            for cat in &self.categories {
                let alias_slices: Vec<&[u8]> = cat.aliases.iter().map(|a| a.to_bytes()).collect();
                if category_alias_list_matches(&alias_slices, name_bytes, c_char::MIN < 0) {
                    return Some(cat);
                }
            }
        }
        None
    }

    fn sync_scalars_to_host(&self) {
        if !self.out_scalars.is_null() {
            unsafe {
                *self.out_scalars = self.scalars;
            }
        }
    }

    fn emit_default_diag(op: c_int, s0: *const c_char) {
        if s0.is_null() {
            return;
        }
        unsafe {
            let bytes = CStr::from_ptr(s0).to_bytes();
            match op {
                0 => {
                    libc::printf(c"%s\n".as_ptr(), s0);
                }
                1 => {
                    libc::printf(c"%s\n".as_ptr(), s0);
                    crate::log::log_active_bytes(2, bytes);
                }
                2 => {
                    libc::printf(c"%s\n".as_ptr(), s0);
                    crate::log::log_active_bytes(1, bytes);
                }
                3 => {
                    crate::log::log_active_bytes(1, bytes);
                }
                _ => {}
            }
        }
    }

    fn emit_host(&self, op: c_int, ints: &[c_int], strs: &[*const c_char], count: usize) {
        if let Some(cb) = self.cb_fn {
            let ints_ptr = if ints.is_empty() {
                std::ptr::null()
            } else {
                ints.as_ptr()
            };
            let strs_ptr = if strs.is_empty() {
                std::ptr::null()
            } else {
                strs.as_ptr()
            };
            unsafe { cb(self.cb_ctx, op, ints_ptr, strs_ptr, count) };
        } else if self.default_diag_sink {
            if let Some(&s0) = strs.first() {
                Self::emit_default_diag(op, s0);
            }
        }
    }

    fn emit_diag(&self, op: c_int, msg: &CStr) {
        let mut trunc = msg.to_bytes().to_vec();
        if trunc.len() > 1023 {
            trunc.truncate(1023);
        }
        let base: &[u8] = match self.strs[0].as_deref() {
            Some(cf) => {
                let raw = cf.to_bytes();
                &raw[crate::paths::base_file_name(raw)..]
            }
            None => b"<noconfig>",
        };
        let line_s = self.config_line.to_string();
        let formatted = format_bytes_msg(&[base, b"(", line_s.as_bytes(), b"): ", &trunc]);
        let ptrs = [formatted.as_ptr()];
        self.emit_host(op, &[], &ptrs, 1);
    }

    #[allow(clippy::too_many_arguments)]
    fn init_daemon(
        &mut self,
        exe_name: &[u8],
        config_filename: Option<&CStr>,
        no_config: bool,
        no_disk_access: bool,
        cmd_opts: &[&CStr],
        flags: c_int,
        version: &CStr,
    ) {
        let disable_curses = (flags & 1) != 0;
        let disable_parcheck = (flags & 2) != 0;
        let disable_tls = (flags & 4) != 0;
        let have_x509_check_host = (flags & 8) != 0;

        self.no_disk_access = no_disk_access;
        self.no_config = no_config;
        self.strs[0] = config_filename.map(CStr::to_owned);

        self.set_option(c"ConfigFile", c"");

        let mut filename = if no_disk_access {
            exe_name.to_vec()
        } else {
            crate::fsops::get_exe_file_name(exe_name)
        };
        crate::paths::normalize_path_separators(&mut filename);
        let app_bin = cstring(filename.clone());
        self.set_option(c"AppBin", &app_bin);
        if let Some(pos) = filename
            .iter()
            .rposition(|&b| b == crate::paths::PATH_SEPARATOR as u8)
        {
            filename.truncate(pos);
        }
        let app_dir = cstring(filename);
        self.set_option(c"AppDir", &app_dir);
        self.strs[1] = Some(app_dir);

        self.set_option(c"Version", version);

        init_defaults_with(disable_curses, |name, val| self.set_option(name, val));

        self.init_opt_file();
        if self.fatal_error {
            return;
        }

        for &opt in cmd_opts {
            self.set_option_string(opt);
        }

        if self.strs[0].is_none() && !no_config {
            let p0 = [c"No configuration-file found".as_ptr()];
            self.emit_host(0, &[], &p0, 1);
            let p1 = [
                c"Please use option \"-c\" or put configuration-file in one of the following locations:"
                    .as_ptr(),
            ];
            self.emit_host(0, &[], &p1, 1);
            for &loc in POSSIBLE_CONFIG_LOCATIONS {
                let loc_c = cstring(loc.to_vec());
                let pl = [loc_c.as_ptr()];
                self.emit_host(0, &[], &pl, 1);
            }
            self.fatal_error = true;
            return;
        }

        for e in &mut self.opt_entries {
            if e.value.is_none() {
                e.value = Some(c"".to_owned());
            }
        }

        self.check_dirs_daemon();
        self.init_options_daemon();

        let mut ct = self.strs[9].clone();
        let web_dir = self.strs[8]
            .as_deref()
            .map_or_else(Vec::new, |s| s.to_bytes().to_vec());
        let upf = self.strs[31].clone();
        let cert_store_empty = self.strs[23]
            .as_deref()
            .map_or(true, |s| s.to_bytes().is_empty());
        let mut scalars = self.scalars;
        check_options_host(
            self,
            &mut scalars,
            self.no_disk_access,
            usize::BITS == 32,
            disable_parcheck,
            disable_curses,
            disable_tls,
            have_x509_check_host,
            cert_store_empty,
            &web_dir,
            &mut ct,
            upf.as_deref(),
        );
        self.scalars = scalars;
        self.sync_scalars_to_host();
        self.strs[9] = ct;

        let mut scalars = self.scalars;
        init_servers_host(self, disable_tls, &mut scalars);
        self.scalars = scalars;
        self.sync_scalars_to_host();

        let dest_dir = self.strs[3].clone();
        let no_disk = self.no_disk_access;
        init_categories_host(self, no_disk, dest_dir.as_deref());
        init_scheduler_host(self);
        init_feeds_host(self);
    }

    fn init_opt_file(&mut self) {
        if self.strs[0].is_none() && !self.no_config {
            let app_dir_bytes = self.strs[1]
                .as_deref()
                .map_or_else(Vec::new, |s| s.to_bytes().to_vec());
            if let Some(found) =
                find_default_config_file(&app_dir_bytes, b'/', |p| self.file_exists(p))
            {
                self.strs[0] = Some(cstring(found));
            }
        }

        if let Some(cf) = self.strs[0].clone() {
            let exp1 = cstring(crate::fsops::expand_file_name(cf.to_bytes()));
            let exp2 = cstring(crate::fsops::expand_home_path_auto(Some(&exp1)));
            self.strs[0] = Some(exp2.clone());
            self.set_option(c"ConfigFile", &exp2);
            self.load_config_file(&exp2);
        }
    }

    fn load_config_file(&mut self, cf: &CStr) {
        self.set_option(c"ConfigFile", cf);
        self.config_line = 0;
        let ok = load_config_file_lines(cf, |line_no, line| {
            self.config_line = line_no;
            self.set_option_string(line);
        });
        if !ok {
            let msg = format_bytes_msg(&[b"Could not open file ", cf.to_bytes()]);
            self.config_error(&msg);
            self.fatal_error = true;
            return;
        }
        self.config_line = 0;
    }

    fn set_option_string(&mut self, option: &CStr) -> bool {
        let Some((optname, optvalue)) = split_and_convert_option_string(option) else {
            let msg = format_bytes_msg(&[b"Invalid option \"", option.to_bytes(), b"\""]);
            self.config_error(&msg);
            return false;
        };
        let check = validate_option_name(&optname, &mut || self.get_option(&optname).is_some());
        match check {
            NameCheck::Valid => {
                self.set_option(&optname, &optvalue);
                true
            }
            NameCheck::Obsolete => {
                let msg = format_bytes_msg(&[
                    b"Option \"",
                    optname.to_bytes(),
                    b"\" is obsolete, ignored",
                ]);
                self.config_warn(&msg);
                true
            }
            NameCheck::ObsoleteLog => {
                let msg = format_bytes_msg(&[
                    b"Option \"",
                    optname.to_bytes(),
                    b"\" is obsolete, ignored, use \"WriteLog\" instead",
                ]);
                self.config_warn(&msg);
                true
            }
            NameCheck::Invalid => {
                let msg = format_bytes_msg(&[b"Invalid option \"", optname.to_bytes(), b"\""]);
                self.config_error(&msg);
                false
            }
        }
    }

    fn check_dirs_daemon(&mut self) {
        self.strs[2] = self.get_option(c"MainDir");
        let main_dir = self.strs[2].clone();
        let no_disk = self.no_disk_access;
        self.strs[3] = check_dir_host(self, no_disk, c"DestDir", main_dir.as_deref(), false, true);
        self.strs[4] = check_dir_host(self, no_disk, c"InterDir", main_dir.as_deref(), true, true);
        self.strs[5] = check_dir_host(self, no_disk, c"TempDir", main_dir.as_deref(), false, true);
        self.strs[6] = check_dir_host(self, no_disk, c"QueueDir", main_dir.as_deref(), false, true);
        self.strs[8] = check_dir_host(self, no_disk, c"WebDir", None, true, false);
        self.strs[7] = check_dir_host(self, no_disk, c"NzbDir", main_dir.as_deref(), false, true);
    }

    fn init_options_daemon(&mut self) {
        self.strs[11] = self.get_option(c"RequiredDir");
        self.strs[9] = self.get_option(c"ConfigTemplate");
        self.strs[14] = self.get_option(c"ControlIP");
        self.strs[15] = self.get_option(c"ControlUsername");
        self.strs[16] = self.get_option(c"ControlPassword");
        self.strs[17] = self.get_option(c"RestrictedUsername");
        self.strs[18] = self.get_option(c"RestrictedPassword");
        self.strs[19] = self.get_option(c"AddUsername");
        self.strs[20] = self.get_option(c"AddPassword");
        self.strs[21] = self.get_option(c"SecureCert");
        self.strs[22] = self.get_option(c"SecureKey");
        self.strs[23] = self.get_option(c"CertStore");
        self.strs[24] = self.get_option(c"AuthorizedIP");
        self.strs[25] = self.get_option(c"LockFile");
        self.strs[26] = self.get_option(c"DaemonUsername");
        self.strs[27] = self.get_option(c"LogFile");
        self.strs[30] = self.get_option(c"UnrarCmd");
        let seven_zip_cmd = self.get_option(c"SevenZipCmd");
        self.strs[31] = self.get_option(c"UnpackPassFile");
        self.strs[32] = self.get_option(c"ExtCleanupDisk");
        self.strs[33] = self.get_option(c"ParIgnoreExt");
        self.strs[35] = self.get_option(c"UnpackIgnoreExt");

        let unrar_bytes = self.strs[30].as_deref().map_or(&[][..], CStr::to_bytes);
        self.strs[38] = Some(if unrar_bytes.is_empty() {
            c"".to_owned()
        } else {
            cstring(resolve_tool_path_option(unrar_bytes))
        });
        let sz_bytes = seven_zip_cmd.as_deref().map_or(&[][..], CStr::to_bytes);
        self.strs[39] = Some(if sz_bytes.is_empty() {
            c"".to_owned()
        } else {
            cstring(resolve_tool_path_option(sz_bytes))
        });

        self.strs[40] = Some(
            self.get_option(c"ShellOverride")
                .unwrap_or_else(|| c"".to_owned()),
        );
        self.strs[34] = self.get_option(c"RenameIgnoreExt");

        let mut mem_path = None;
        let mut hl_ext = None;
        let mut dupe_url = None;
        let mut dupe_key = None;
        let mut warn_no_url = false;
        self.scalars = init_scalar_options_host(
            self,
            &mut mem_path,
            &mut hl_ext,
            &mut dupe_url,
            &mut dupe_key,
            &mut warn_no_url,
        );
        self.strs[36] = mem_path;
        self.strs[37] = hl_ext;
        self.strs[12] = dupe_url;
        self.strs[13] = dupe_key;
        self.sync_scalars_to_host();

        if warn_no_url {
            let p = [c"Option <DupeSearch> is enabled but option <DupeSearchUrl> is empty: no duplicates will be searched".as_ptr()];
            self.emit_host(3, &[], &p, 1);
        }
    }

    pub(crate) fn has_config_errors(&self) -> bool {
        self.config_errors.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl OptionsHost for NzbgetRsOptions {
    fn find_entry(&mut self, name: &CStr) -> Option<(Option<CString>, Option<CString>, i32)> {
        let entry = self.opt_entries.iter_mut().find(|e| eq(&e.name, name))?;
        if entry.name.to_bytes() != name.to_bytes() {
            entry.name = name.to_owned();
        }
        Some((entry.value.clone(), entry.def_value.clone(), entry.line_no))
    }

    fn set_entry_raw_value(&mut self, name: &CStr, value: &CStr) {
        if let Some(entry) = self.opt_entries.iter_mut().find(|e| eq(&e.name, name)) {
            if entry.name.to_bytes() != name.to_bytes() {
                entry.name = name.to_owned();
            }
            entry.set_value(value);
        }
    }

    fn set_option(&mut self, name: &CStr, value: &CStr) {
        let idx = match self.opt_entries.iter().position(|e| eq(&e.name, name)) {
            Some(i) => {
                if self.opt_entries[i].name.to_bytes() != name.to_bytes() {
                    self.opt_entries[i].name = name.to_owned();
                }
                i
            }
            None => {
                self.opt_entries.push(RustOptEntry::new(name, None));
                self.opt_entries.len() - 1
            }
        };
        let curvalue =
            if options_should_expand_home_value(Some(value.to_bytes()), self.no_disk_access) {
                crate::fsops::expand_home_path_auto(Some(value))
            } else {
                value.to_bytes().to_vec()
            };
        self.opt_entries[idx].line_no = self.config_line;
        let expanded = expand_option_variables(&curvalue, |var_name| {
            self.get_option(var_name).map(|c| c.into_bytes())
        });
        let expanded_cstr = cstring(expanded);
        self.opt_entries[idx].set_value(&expanded_cstr);
    }

    fn set_config_line(&mut self, line: i32) {
        self.config_line = line;
    }

    fn get_config_line(&self) -> i32 {
        self.config_line
    }

    fn config_error(&mut self, msg: &CStr) {
        self.emit_diag(1, msg);
        self.config_errors
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn config_warn(&mut self, msg: &CStr) {
        self.emit_diag(2, msg);
    }

    fn file_exists(&self, path: &CStr) -> bool {
        crate::fsops::file_exists(path.to_bytes(), &crate::fsops::posix_stat)
    }

    fn ensure_directory(&mut self, dir: &CStr) -> Result<(), CString> {
        let mut msg = Vec::new();
        if !crate::fsops::force_directories(dir.to_bytes(), &mut msg, &crate::fsops::posix_stat) {
            return Err(cstring(msg));
        }
        #[cfg(unix)]
        unsafe {
            if libc::getuid() == 0 || libc::geteuid() == 0 {
                if let Some(user) = self.get_option(c"DaemonUsername") {
                    let pwd = libc::getpwnam(user.as_ptr());
                    if !pwd.is_null() {
                        let _ = libc::chown(dir.as_ptr(), (*pwd).pw_uid, (*pwd).pw_gid);
                    }
                }
            }
        }
        Ok(())
    }

    fn add_news_server(
        &mut self,
        id: i32,
        active: bool,
        name: Option<&CStr>,
        host: Option<&CStr>,
        port: i32,
        ip_version: i32,
        user: Option<&CStr>,
        pass: Option<&CStr>,
        join_group: bool,
        tls: bool,
        cipher: Option<&CStr>,
        max_connections: i32,
        retention: i32,
        level: i32,
        group: i32,
        optional: bool,
        cert_verif_level: u32,
    ) {
        if self.daemon_subsystems {
            #[cfg(unix)]
            if let Some(pool) = crate::net::active_server_pool() {
                unsafe {
                    crate::net::nzbget_rs_serverpool_add(
                        std::sync::Arc::as_ptr(&pool),
                        id,
                        c_int::from(active),
                        name.map_or(std::ptr::null(), CStr::as_ptr),
                        host.map_or(std::ptr::null(), CStr::as_ptr),
                        port,
                        ip_version,
                        user.map_or(std::ptr::null(), CStr::as_ptr),
                        pass.map_or(std::ptr::null(), CStr::as_ptr),
                        c_int::from(join_group),
                        c_int::from(tls),
                        cipher.map_or(std::ptr::null(), CStr::as_ptr),
                        max_connections,
                        retention,
                        level,
                        group,
                        c_int::from(optional),
                        cert_verif_level,
                    );
                }
            }
            return;
        }
        let ints = [
            id,
            c_int::from(active),
            port,
            ip_version,
            c_int::from(join_group),
            c_int::from(tls),
            max_connections,
            retention,
            level,
            group,
            c_int::from(optional),
            cert_verif_level as c_int,
        ];
        let ptrs = [
            name.map_or(std::ptr::null(), CStr::as_ptr),
            host.map_or(std::ptr::null(), CStr::as_ptr),
            user.map_or(std::ptr::null(), CStr::as_ptr),
            pass.map_or(std::ptr::null(), CStr::as_ptr),
            cipher.map_or(std::ptr::null(), CStr::as_ptr),
        ];
        self.emit_host(7, &ints, &ptrs, 5);
    }

    fn add_feed(
        &mut self,
        id: i32,
        name: Option<&CStr>,
        url: Option<&CStr>,
        interval: i32,
        filter: Option<&CStr>,
        backlog: bool,
        pause_nzb: bool,
        category: Option<&CStr>,
        category_source: i32,
        priority: i32,
        cert_verif_level: u32,
    ) {
        if self.daemon_subsystems {
            if let Some(fc) = crate::feed::active_feed_coord() {
                fc.add_feed_direct(
                    id,
                    name.map_or(&[][..], CStr::to_bytes),
                    url.map_or(&[][..], CStr::to_bytes),
                    backlog,
                    interval,
                    filter.map_or(&[][..], CStr::to_bytes),
                    pause_nzb,
                    category.map_or(&[][..], CStr::to_bytes),
                    category_source,
                    priority,
                    cert_verif_level,
                );
            }
            return;
        }
        let ints = [
            id,
            interval,
            c_int::from(backlog),
            c_int::from(pause_nzb),
            category_source,
            priority,
            cert_verif_level as c_int,
        ];
        let ptrs = [
            name.map_or(std::ptr::null(), CStr::as_ptr),
            url.map_or(std::ptr::null(), CStr::as_ptr),
            filter.map_or(std::ptr::null(), CStr::as_ptr),
            category.map_or(std::ptr::null(), CStr::as_ptr),
            // formerly FeedN.Extensions
            std::ptr::null(),
        ];
        self.emit_host(8, &ints, &ptrs, 5);
    }

    fn add_task(
        &mut self,
        id: i32,
        hours: i32,
        minutes: i32,
        week_days_bits: i32,
        command: i32,
        param: Option<&CStr>,
    ) {
        if self.daemon_subsystems {
            if let Some(sched) = crate::scheduler::active_scheduler() {
                sched.add_task(
                    id,
                    hours,
                    minutes,
                    week_days_bits,
                    command,
                    param.map_or(&[][..], CStr::to_bytes),
                );
            }
            return;
        }
        let ints = [id, hours, minutes, week_days_bits, command];
        let ptrs = [param.map_or(std::ptr::null(), CStr::as_ptr)];
        self.emit_host(9, &ints, &ptrs, 1);
    }

    fn add_category(
        &mut self,
        name: &CStr,
        dest_dir: Option<&CStr>,
        unpack: bool,
        aliases: &[CString],
    ) {
        self.categories.push(RustCategory {
            name: name.to_owned(),
            dest_dir: dest_dir.unwrap_or(c"").to_owned(),
            unpack,
            aliases: aliases.to_vec(),
        });
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_new(
    exe_name: *const c_char,
    config_filename: *const c_char,
    no_config: c_int,
    no_disk_access: c_int,
    cmd_opts: *const *const c_char,
    cmd_opts_len: usize,
    flags: c_int,
    version: *const c_char,
    cb_ctx: *mut std::ffi::c_void,
    cb_fn: NzbgetRsOptionsHostFn,
    out_scalars: *mut NzbgetRsParsedOptions,
    out_state_flags: *mut c_int,
) -> *mut NzbgetRsOptions {
    #[cfg(unix)]
    let daemon_snap = if no_config < 0 {
        crate::lifecycle::daemon_cmdline_options_inputs()
    } else {
        None
    };
    #[cfg(not(unix))]
    let daemon_snap: Option<(Vec<u8>, Option<CString>, bool, Vec<CString>)> = None;

    let (exe_bytes, cf_opt, eff_no_config, cmd_vec, eff_daemon_subsys): (
        &[u8],
        Option<&CStr>,
        bool,
        Vec<&CStr>,
        bool,
    ) = if let Some((ref exe_v, ref cf_v, nc, ref c_list)) = daemon_snap {
        (
            exe_v.as_slice(),
            cf_v.as_deref(),
            nc,
            c_list.iter().map(CString::as_c_str).collect(),
            true,
        )
    } else {
        let eb = if exe_name.is_null() {
            &[][..]
        } else {
            CStr::from_ptr(exe_name).to_bytes()
        };
        let cf = if config_filename.is_null() {
            None
        } else {
            Some(CStr::from_ptr(config_filename))
        };
        let mut cv = Vec::with_capacity(cmd_opts_len);
        if !cmd_opts.is_null() {
            for i in 0..cmd_opts_len {
                let p = *cmd_opts.add(i);
                if !p.is_null() {
                    cv.push(CStr::from_ptr(p));
                }
            }
        }
        (eb, cf, no_config != 0, cv, (flags & 16) != 0)
    };
    let ver_cstr = if version.is_null() {
        crate::ffi::build_version_cstr()
    } else {
        CStr::from_ptr(version)
    };
    let mut opts = NzbgetRsOptions {
        opt_entries: Vec::new(),
        categories: Vec::new(),
        strs: std::array::from_fn(|_| None),
        no_disk_access: no_disk_access != 0,
        no_config: eff_no_config,
        daemon_subsystems: eff_daemon_subsys,
        default_diag_sink: (flags & 32) != 0 || daemon_snap.is_some(),
        fatal_error: false,
        config_errors: std::sync::atomic::AtomicBool::new(false),
        mode_flags: std::sync::atomic::AtomicU8::new(0),
        config_line: 0,
        scalars: NzbgetRsParsedOptions::with_defaults(),
        out_scalars,
        cb_ctx,
        cb_fn,
    };
    opts.sync_scalars_to_host();
    opts.init_daemon(
        exe_bytes,
        cf_opt,
        eff_no_config,
        no_disk_access != 0,
        &cmd_vec,
        flags & !(16 | 32),
        ver_cstr,
    );
    opts.sync_scalars_to_host();
    if !out_state_flags.is_null() {
        let had_err = opts.config_errors.load(std::sync::atomic::Ordering::SeqCst);
        *out_state_flags = c_int::from(opts.fatal_error) | (c_int::from(had_err) << 1);
    }
    opts.out_scalars = std::ptr::null_mut();
    opts.cb_ctx = std::ptr::null_mut();
    opts.cb_fn = None;
    let arc = std::sync::Arc::new(opts);
    if let Ok(mut g) = ACTIVE_OPTIONS.write() {
        *g = Some(std::sync::Arc::downgrade(&arc));
    }
    std::sync::Arc::into_raw(arc).cast_mut()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_free(handle: *mut NzbgetRsOptions) {
    if !handle.is_null() {
        let arc = std::sync::Arc::from_raw(handle.cast_const());
        let wk = std::sync::Arc::downgrade(&arc);
        if let Ok(mut g) = ACTIVE_OPTIONS.write() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
            {
                *g = None;
            }
        }
        drop(arc);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_str(
    handle: *const NzbgetRsOptions,
    id: c_int,
) -> *const c_char {
    if handle.is_null() || id < 0 || (id as usize) >= OPT_STR_COUNT {
        return std::ptr::null();
    }
    (*handle).strs[id as usize]
        .as_deref()
        .map_or(std::ptr::null(), CStr::as_ptr)
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_query(
    handle: *const NzbgetRsOptions,
    op: c_int,
    s1: *const c_char,
    a1: c_int,
    out_int: *mut c_int,
    out_strs: *mut *const c_char,
    ctx: usize,
    cb: Option<unsafe extern "C" fn(usize, *const c_char, *const c_char, c_int)>,
) -> c_int {
    let Some(opts) = handle.as_ref() else {
        return 0;
    };
    match op {
        0 => {
            if s1.is_null() {
                return 0;
            }
            let Some(cat) = opts.find_category(CStr::from_ptr(s1), a1 != 0) else {
                return 0;
            };
            if !out_int.is_null() {
                *out_int = c_int::from(cat.unpack);
            }
            if !out_strs.is_null() {
                *out_strs.add(0) = cat.name.as_ptr();
                *out_strs.add(1) = cat.dest_dir.as_ptr();
                // formerly CategoryN.Extensions
                *out_strs.add(2) = c"".as_ptr();
            }
            1
        }
        1 => {
            if let Some(f) = cb {
                let signed_char = c_char::MIN < 0;
                for e in &opts.opt_entries {
                    let val = e.value.as_deref().unwrap_or(c"");
                    let restr = c_int::from(option_restricted(&e.name, signed_char));
                    f(ctx, e.name.as_ptr(), val.as_ptr(), restr);
                }
            }
            opts.opt_entries.len() as c_int
        }
        2 => {
            if let Some(f) = cb {
                for cat in &opts.categories {
                    f(
                        ctx,
                        cat.name.as_ptr(),
                        c"".as_ptr(),
                        c_int::from(cat.unpack),
                    );
                }
            }
            opts.categories.len() as c_int
        }
        4 => {
            let bit = 1u8 << ((a1 & 3) as u8);
            if (a1 & 0x10) != 0 {
                if (a1 & 0x20) != 0 {
                    opts.mode_flags
                        .fetch_or(bit, std::sync::atomic::Ordering::SeqCst);
                } else {
                    opts.mode_flags
                        .fetch_and(!bit, std::sync::atomic::Ordering::SeqCst);
                }
            }
            c_int::from((opts.mode_flags.load(std::sync::atomic::Ordering::SeqCst) & bit) != 0)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod options_full_suite_port_from_cpp {
    use super::*;

    #[derive(Default)]
    struct OptionsExtenderMock {
        news_servers: i32,
        feeds: i32,
        tasks: i32,
        category_sources: Vec<i32>,
        cert_verif_levels: Vec<u32>,
    }

    impl RustOptionsExtender for OptionsExtenderMock {
        fn add_news_server(
            &mut self,
            _id: i32,
            _active: bool,
            _name: Option<&CStr>,
            _host: Option<&CStr>,
            _port: i32,
            _ip_version: i32,
            _user: Option<&CStr>,
            _pass: Option<&CStr>,
            _join_group: bool,
            _tls: bool,
            _cipher: Option<&CStr>,
            _max_connections: i32,
            _retention: i32,
            _level: i32,
            _group: i32,
            _optional: bool,
            _cert_verif_level: u32,
        ) {
            self.news_servers += 1;
        }

        fn add_feed(
            &mut self,
            _id: i32,
            _name: Option<&CStr>,
            _url: Option<&CStr>,
            _interval: i32,
            _filter: Option<&CStr>,
            _backlog: bool,
            _pause_nzb: bool,
            _category: Option<&CStr>,
            category_source: i32,
            _priority: i32,
            cert_verif_level: u32,
        ) {
            self.feeds += 1;
            self.category_sources.push(category_source);
            self.cert_verif_levels.push(cert_verif_level);
        }

        fn add_task(
            &mut self,
            _id: i32,
            _hours: i32,
            _minutes: i32,
            _week_days_bits: i32,
            _command: i32,
            _param: Option<&CStr>,
        ) {
            self.tasks += 1;
        }
    }

    #[test]
    fn options_init_without_configuration_file_test() {
        let options = RustOptions::new_no_config(None, None);
        assert!(options.config_filename.is_none());
        assert_eq!(options.temp_dir.as_c_str(), c"~/downloads/tmp");
    }

    #[test]
    fn passing_command_line_options() {
        let cmd_opts = [
            c"ControlUsername=my-user-name-1",
            c"ControlUsername=my-user-name-2",
        ];
        let options = RustOptions::new_no_config(Some(&cmd_opts), None);
        assert!(options.config_filename.is_none());
        assert_eq!(options.control_username.as_c_str(), c"my-user-name-2");
    }

    #[test]
    fn calling_extender() {
        let cmd_opts = [
            c"Server1.Host=news.mynewsserver.com",
            c"Server1.Port=119",
            c"Server1.Connections=4",
            c"Server2.Host=news1.mynewsserver.com",
            c"Server2.Port=563",
            c"Server2.Connections=2",
            c"Feed1.Url=http://my.feed.com",
            c"Task1.Time=*:00",
            c"Task1.WeekDays=1-7",
            c"Task1.Command=pausedownload",
        ];
        let mut extender = OptionsExtenderMock::default();
        let _options = RustOptions::new_no_config(Some(&cmd_opts), Some(&mut extender));

        assert_eq!(extender.news_servers, 2);
        assert_eq!(extender.feeds, 1);
        assert_eq!(extender.tasks, 24);
        assert_eq!(extender.category_sources[0], 1); // NZBFile
    }

    #[test]
    fn dupe_search_options_defaults_test() {
        let mut extender = OptionsExtenderMock::default();
        let options = RustOptions::new_no_config(Some(&[]), Some(&mut extender));

        assert_eq!(options.scalars.dupe_search, 0);
        assert_eq!(options.dupe_search_url.as_c_str(), c"");
        assert_eq!(options.dupe_search_api_key.as_c_str(), c"");
        assert_eq!(options.scalars.dupe_search_delay, 20);
        assert_eq!(options.scalars.dupe_search_max_donors, 0);
        assert_eq!(options.scalars.dupe_health_percent, 5);
        assert_eq!(options.scalars.dupe_health_min, 50);
        assert_eq!(options.scalars.dupe_health_max, 1000);
        assert_eq!(options.scalars.dupe_body_checks, 20);
        assert_eq!(options.scalars.dupe_health_budget, 120);
        assert_eq!(options.scalars.dupe_min_alive, 50);
        assert_eq!(options.scalars.dupe_fast_donors, 5);
        assert_eq!(options.scalars.dupe_primary_score, 1_000_000);
        assert_eq!(options.scalars.dupe_size_tolerance, 0);
        assert_eq!(options.scalars.dupe_body_percent, 20);
        assert_eq!(options.scalars.dupe_check_concurrency, 10);
        assert_eq!(options.scalars.dupe_check_connections, 20);
    }

    #[test]
    fn dupe_search_options_parse_test() {
        let cmd_opts = [
            c"DupeSearch=yes",
            c"DupeSearchUrl=http://127.0.0.1:5076/api",
            c"DupeSearchApiKey=secret",
            c"DupeSearchDelay=7",
            c"DupeSearchMaxDonors=3",
            c"DupeHealthPercent=10",
            c"DupeHealthMin=30",
            c"DupeHealthMax=500",
            c"DupeBodyChecks=0",
            c"DupeHealthBudget=60",
            c"DupeMinAlive=70",
            c"DupeFastDonors=2",
            c"DupePrimaryScore=500000",
            c"DupeSizeTolerance=20",
            c"DupeBodyPercent=5",
            c"DupeCheckConcurrency=3",
            c"DupeCheckConnections=8",
        ];
        let mut extender = OptionsExtenderMock::default();
        let options = RustOptions::new_no_config(Some(&cmd_opts), Some(&mut extender));

        assert_eq!(options.scalars.dupe_search, 1);
        assert_eq!(
            options.dupe_search_url.as_c_str(),
            c"http://127.0.0.1:5076/api"
        );
        assert_eq!(options.dupe_search_api_key.as_c_str(), c"secret");
        assert_eq!(options.scalars.dupe_search_delay, 7);
        assert_eq!(options.scalars.dupe_search_max_donors, 3);
        assert_eq!(options.scalars.dupe_health_percent, 10);
        assert_eq!(options.scalars.dupe_health_min, 30);
        assert_eq!(options.scalars.dupe_health_max, 500);
        assert_eq!(options.scalars.dupe_body_checks, 0);
        assert_eq!(options.scalars.dupe_health_budget, 60);
        assert_eq!(options.scalars.dupe_min_alive, 70);
        assert_eq!(options.scalars.dupe_fast_donors, 2);
        assert_eq!(options.scalars.dupe_primary_score, 500_000);
        assert_eq!(options.scalars.dupe_size_tolerance, 20);
        assert_eq!(options.scalars.dupe_body_percent, 5);
        assert_eq!(options.scalars.dupe_check_concurrency, 3);
        assert_eq!(options.scalars.dupe_check_connections, 8);
    }

    #[test]
    fn dupe_search_options_clamp_test() {
        let cmd_opts = [
            c"DupeSearch=yes",
            c"DupeSearchUrl=http://127.0.0.1:5076/api",
            c"DupeSearchDelay=-5",
            c"DupeSearchMaxDonors=-1",
            c"DupeHealthPercent=500",
            c"DupeHealthMin=0",
            c"DupeHealthMax=10",
            c"DupeBodyChecks=-3",
            c"DupeHealthBudget=1",
            c"DupeMinAlive=150",
            c"DupeFastDonors=-2",
            c"DupePrimaryScore=5",
            c"DupeSizeTolerance=-1",
            c"DupeBodyPercent=400",
            c"DupeCheckConcurrency=0",
            c"DupeCheckConnections=0",
        ];
        let mut extender = OptionsExtenderMock::default();
        let options = RustOptions::new_no_config(Some(&cmd_opts), Some(&mut extender));

        assert_eq!(options.scalars.dupe_search_delay, 0);
        assert_eq!(options.scalars.dupe_search_max_donors, 0);
        assert_eq!(options.scalars.dupe_health_percent, 100);
        assert_eq!(options.scalars.dupe_health_min, 1);
        assert_eq!(options.scalars.dupe_health_max, 10);
        assert_eq!(options.scalars.dupe_body_checks, 0);
        assert_eq!(options.scalars.dupe_health_budget, 5);
        assert_eq!(options.scalars.dupe_min_alive, 100);
        assert_eq!(options.scalars.dupe_fast_donors, 0);
        assert_eq!(options.scalars.dupe_primary_score, 1000);
        assert_eq!(options.scalars.dupe_size_tolerance, 0);
        assert_eq!(options.scalars.dupe_body_percent, 100);
        assert_eq!(options.scalars.dupe_check_concurrency, 1);
        assert_eq!(options.scalars.dupe_check_connections, 1);

        let cmd_opts2 = [c"DupeHealthMin=300", c"DupeHealthMax=100"];
        let options2 = RustOptions::new_no_config(Some(&cmd_opts2), Some(&mut extender));
        assert_eq!(options2.scalars.dupe_health_min, 300);
        assert_eq!(options2.scalars.dupe_health_max, 300);
    }

    #[test]
    fn dupe_search_without_url_stays_off_test() {
        let cmd_opts = [c"DupeSearch=yes"];
        let mut extender = OptionsExtenderMock::default();
        let options = RustOptions::new_no_config(Some(&cmd_opts), Some(&mut extender));

        assert_eq!(options.scalars.dupe_search, 0);
        assert_eq!(options.dupe_search_url.as_c_str(), c"");
    }

    #[test]
    fn parse_category_source_test() {
        let cmd_opts = [
            c"Feed1.Url=http://my.feed.com",
            c"Feed1.CategorySource=Auto",
            c"Feed2.Url=http://my.feed2.com",
            c"Feed2.CategorySource=NZBFile",
            c"Feed3.Url=http://my.feed3.com",
            c"Feed3.CategorySource=FeedFile",
            c"Feed4.Url=http://my.feed4.com",
            c"Feed4.CategorySource=auto",
            c"Feed5.Url=http://my.feed5.com",
            c"Feed5.CategorySource=nzbfile",
            c"Feed6.Url=http://my.feed6.com",
            c"Feed6.CategorySource=feedfile",
            c"Feed7.Url=http://my.feed7.com",
            c"Feed7.CategorySource=auto and nzbfile and feedfile",
        ];
        let mut extender = OptionsExtenderMock::default();
        let _options = RustOptions::new_no_config(Some(&cmd_opts), Some(&mut extender));

        assert_eq!(extender.category_sources, vec![0, 1, 2, 0, 1, 2, 0]);
    }

    #[test]
    fn parse_paths_test() {
        let cmd_opts = [
            c"MainDir=/usr/etc/nzbget",
            c"DestDir=/usr/etc/nzbget/dst",
            c"InterDir=/usr/etc/nzbget/inter",
            c"TempDir=/usr/etc/nzbget/tmp",
            c"WebDir=/usr/etc/nzbget/webui",
            c"NzbDir=/usr/etc/nzbget/nzb",
            c"QueueDir=/usr/etc/nzbget/queue",
            c"LockFile=/usr/etc/nzbget/nzbget.lock",
            c"LogFile=/usr/etc/nzbget/nzbget.log",
            c"ConfigTemplate=/usr/etc/nzbget/nzbget.template.conf",
            c"CertStore=/usr/etc/nzbget/cacert.pem",
            c"SecureCert=/usr/etc/nzbget/cert.pem",
            c"SecureKey=/usr/etc/nzbget/key.pem",
            c"UnpackPassFile=/usr/etc/nzbget/unpackpass",
        ];
        let mut extender = OptionsExtenderMock::default();
        let options = RustOptions::new_no_config(Some(&cmd_opts), Some(&mut extender));

        assert_eq!(options.main_dir.as_c_str(), c"/usr/etc/nzbget");
        assert_eq!(options.dest_dir.as_c_str(), c"/usr/etc/nzbget/dst");
        assert_eq!(options.inter_dir.as_c_str(), c"/usr/etc/nzbget/inter");
        assert_eq!(options.temp_dir.as_c_str(), c"/usr/etc/nzbget/tmp");
        assert_eq!(options.web_dir.as_c_str(), c"/usr/etc/nzbget/webui");
        assert_eq!(options.nzb_dir.as_c_str(), c"/usr/etc/nzbget/nzb");
        assert_eq!(options.queue_dir.as_c_str(), c"/usr/etc/nzbget/queue");
        assert_eq!(options.lock_file.as_c_str(), c"/usr/etc/nzbget/nzbget.lock");
        assert_eq!(options.log_file.as_c_str(), c"/usr/etc/nzbget/nzbget.log");
        assert_eq!(
            options.config_template.as_c_str(),
            c"/usr/etc/nzbget/nzbget.template.conf"
        );
        assert_eq!(options.cert_store.as_c_str(), c"/usr/etc/nzbget/cacert.pem");
        assert_eq!(options.secure_cert.as_c_str(), c"/usr/etc/nzbget/cert.pem");
        assert_eq!(options.secure_key.as_c_str(), c"/usr/etc/nzbget/key.pem");
        assert_eq!(
            options.unpack_pass_file.as_c_str(),
            c"/usr/etc/nzbget/unpackpass"
        );
    }

    #[test]
    fn parse_tool_paths_test() {
        {
            let cmd_opts = [c"UnrarCmd=/test/path/unrar", c"SevenZipCmd=/test/path/7z"];
            let mut extender = OptionsExtenderMock::default();
            let options = RustOptions::new_no_config(Some(&cmd_opts), Some(&mut extender));
            assert_eq!(options.unrar_path.as_c_str(), c"/test/path/unrar");
            assert_eq!(options.seven_zip_path.as_c_str(), c"/test/path/7z");
        }
        {
            let cmd_opts = [
                c"UnrarCmd='C:\\Test Path\\unrar.exe' -y",
                c"SevenZipCmd='C:\\Test Path\\7z.exe' -y",
            ];
            let mut extender = OptionsExtenderMock::default();
            let options = RustOptions::new_no_config(Some(&cmd_opts), Some(&mut extender));
            assert_eq!(options.unrar_path.as_c_str(), c"C:\\Test Path\\unrar.exe");
            assert_eq!(options.seven_zip_path.as_c_str(), c"C:\\Test Path\\7z.exe");
        }
    }

    #[test]
    fn parse_feed_cert_verification_test() {
        let cmd_opts = [
            c"Feed1.Url=http://my.feed.com",
            c"Feed1.CertVerification=None",
            c"Feed2.Url=http://my.feed2.com",
            c"Feed2.CertVerification=Minimal",
            c"Feed3.Url=http://my.feed3.com",
            c"Feed3.CertVerification=Strict",
            c"Feed4.Url=http://my.feed4.com",
            c"Feed4.CertVerification=none",
            c"Feed5.Url=http://my.feed5.com",
        ];
        let mut extender = OptionsExtenderMock::default();
        let _options = RustOptions::new_no_config(Some(&cmd_opts), Some(&mut extender));

        assert_eq!(extender.cert_verif_levels, vec![0, 1, 2, 0, 2]);
    }

    #[test]
    fn dupe_article_fallback_option_test() {
        {
            let options = RustOptions::new_no_config(Some(&[]), None);
            assert_eq!(options.scalars.dupe_article_fallback, 0); // dafNone
        }
        {
            let options = RustOptions::new_no_config(Some(&[c"DupeArticleFallback=article"]), None);
            assert_eq!(options.scalars.dupe_article_fallback, 1); // dafArticle
        }
        {
            let options = RustOptions::new_no_config(Some(&[c"DupeArticleFallback=stream"]), None);
            assert_eq!(options.scalars.dupe_article_fallback, 2); // dafStream
        }
        {
            let options = RustOptions::new_no_config(Some(&[c"DupeArticleFallback=yes"]), None);
            assert_eq!(options.scalars.dupe_article_fallback, 1); // dafArticle
        }
    }

    #[test]
    fn health_check_dupe_option_test() {
        {
            let options = RustOptions::new_no_config(Some(&[c"HealthCheck=dupe"]), None);
            assert_eq!(options.scalars.health_check, 4); // hcDupe
        }
        {
            let options = RustOptions::new_no_config(Some(&[c"HealthCheck=park"]), None);
            assert_eq!(options.scalars.health_check, 2); // hcPark
        }
    }

    #[test]
    fn dupe_stream_decompress_option_test() {
        {
            let options = RustOptions::new_no_config(Some(&[]), None);
            assert_eq!(options.scalars.dupe_stream_decompress, 0);
        }
        {
            let options = RustOptions::new_no_config(Some(&[c"DupeStreamDecompress=yes"]), None);
            assert_eq!(options.scalars.dupe_stream_decompress, 1);
        }
    }

    #[test]
    fn dupe_stream_timeout_option_test() {
        {
            let options = RustOptions::new_no_config(Some(&[]), None);
            assert_eq!(options.scalars.dupe_stream_timeout, 60);
        }
        {
            let options = RustOptions::new_no_config(Some(&[c"DupeStreamTimeout=0"]), None);
            assert_eq!(options.scalars.dupe_stream_timeout, 0);
        }
        {
            let options = RustOptions::new_no_config(Some(&[c"DupeStreamTimeout=-5"]), None);
            assert_eq!(options.scalars.dupe_stream_timeout, 0);
        }
    }

    #[test]
    fn non_utf8_diagnostics_and_long_paths_preserved() {
        let raw_bad_enum = c"bad\xff\xfeenum";
        let msg = format_bytes_msg(&[
            b"Invalid value for option \"",
            b"ParCheck",
            b"\": \"",
            raw_bad_enum.to_bytes(),
            b"\"",
        ]);
        assert_eq!(
            msg.to_bytes(),
            b"Invalid value for option \"ParCheck\": \"bad\xff\xfeenum\""
        );

        let long_seg = "a".repeat(2500);
        let cat_dest_opt = cstring(format!("Category1.DestDir=/{long_seg}").into_bytes());
        let opts = [c"Category1.Name=movies", cat_dest_opt.as_c_str()];
        let options = RustOptions::new_no_config(Some(&opts), None);
        assert_eq!(options.categories.len(), 1);
        assert_eq!(options.categories[0].dest_dir.to_bytes().len(), 2501);

        let long_web_dir = format!("/{long_seg}").into_bytes();
        let default_conf =
            options_format_default_conf_path(&long_web_dir, crate::paths::PATH_SEPARATOR);
        assert_eq!(default_conf.len(), 2501 + "/nzbget.conf".len());
    }

    #[test]
    fn find_default_and_load_config_file_test() {
        let found =
            find_default_config_file(b"/opt/app", b'/', |p| p.to_bytes() == b"/etc/rustyget.conf");
        assert_eq!(found.as_deref(), Some(b"/etc/rustyget.conf".as_slice()));

        let tmp =
            std::env::temp_dir().join(format!("rustyget_opts_test_{}.conf", std::process::id()));
        std::fs::write(
            &tmp,
            b"# comment\n\nControlPort=6789\r\n  \nMainDir=/tmp/rg\n",
        )
        .unwrap();
        let c_tmp = CString::new(tmp.to_str().unwrap()).unwrap();
        let mut lines = Vec::new();
        assert!(load_config_file_lines(&c_tmp, |ln, s| {
            lines.push((ln, s.to_bytes().to_vec()));
        }));
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(
            lines,
            vec![
                (3, b"ControlPort=6789".to_vec()),
                (5, b"MainDir=/tmp/rg".to_vec())
            ]
        );
    }

    #[test]
    fn options_new_preserves_startup_and_parsed_targets_for_init_diagnostics() {
        struct Cap {
            scalars: NzbgetRsParsedOptions,
            events: Vec<(c_int, i32, i32, String)>,
        }
        unsafe extern "C" fn cb(
            ctx: *mut std::ffi::c_void,
            op: c_int,
            _ints: *const c_int,
            strs: *const *const c_char,
            _count: usize,
        ) {
            if (1..=3).contains(&op) {
                let cap = &mut *(ctx as *mut Cap);
                let msg = std::str::from_utf8(CStr::from_ptr(*strs).to_bytes())
                    .unwrap()
                    .to_owned();
                cap.events.push((
                    op,
                    cap.scalars.error_target,
                    cap.scalars.warning_target,
                    msg,
                ));
            }
        }

        let mut cap = Cap {
            scalars: NzbgetRsParsedOptions::default(),
            events: Vec::new(),
        };
        let cmd_opts = [
            c"BogusEarlyOption=1".as_ptr(),
            c"ErrorTarget=both".as_ptr(),
            c"WarningTarget=log".as_ptr(),
            c"DupeSearch=yes".as_ptr(),
            c"DupeSearchUrl=".as_ptr(),
            c"Server1.Host=news.example.com".as_ptr(),
        ];
        let mut state_flags: c_int = 0;
        let scalars_ptr: *mut NzbgetRsParsedOptions = &mut cap.scalars;
        let handle = unsafe {
            nzbget_rs_options_new(
                c"rustyget".as_ptr(),
                std::ptr::null(),
                1,
                1,
                cmd_opts.as_ptr(),
                cmd_opts.len(),
                0,
                c"1.0".as_ptr(),
                (&mut cap as *mut Cap).cast(),
                Some(cb),
                scalars_ptr,
                &mut state_flags,
            )
        };
        unsafe { nzbget_rs_options_free(handle) };

        assert_eq!(state_flags & 2, 2);
        // 1) Pre-InitOptions invalid option sees startup default targets (mtScreen = 1).
        assert_eq!(cap.events[0].0, 1);
        assert_eq!(cap.events[0].1, 1);
        assert_eq!(cap.events[0].2, 1);
        assert!(cap.events[0]
            .3
            .contains("Invalid option \"BogusEarlyOption\""));
        // 2) DupeSearch-without-URL warning sees parsed targets (ErrorTarget=both=3, WarningTarget=log=2).
        let dupe_ev = cap
            .events
            .iter()
            .find(|e| e.0 == 3)
            .expect("expected DupeSearch warning");
        assert_eq!(dupe_ev.1, 3);
        assert_eq!(dupe_ev.2, 2);
        // 3) Post-InitOptions incomplete Server1 error also sees parsed targets.
        let srv_ev = cap
            .events
            .iter()
            .find(|e| e.3.contains("Server definition not complete"))
            .expect("expected Server1 error");
        assert_eq!(srv_ev.0, 1);
        assert_eq!(srv_ev.1, 3);
        assert_eq!(srv_ev.2, 2);
    }

    #[test]
    fn options_query_serves_entries_and_categories() {
        assert!(option_restricted(c"Server1.Password", true));
        assert!(!option_restricted(c"Server1.Host", true));

        let cmd_opts = [
            c"Server1.Host=news.example.com".as_ptr(),
            c"Server1.Port=119".as_ptr(),
            c"Server1.Connections=2".as_ptr(),
            c"Server1.Password=secret".as_ptr(),
            c"Category1.Name=Movies".as_ptr(),
            c"Category1.DestDir=/dl/movies".as_ptr(),
            c"Category1.Unpack=no".as_ptr(),
            c"Category1.Aliases=Films*;HD*".as_ptr(),
        ];
        let mut scalars = NzbgetRsParsedOptions::default();
        let mut state_flags: c_int = 0;
        let handle = unsafe {
            nzbget_rs_options_new(
                c"rustyget".as_ptr(),
                std::ptr::null(),
                1,
                1,
                cmd_opts.as_ptr(),
                cmd_opts.len(),
                0,
                c"1.0".as_ptr(),
                std::ptr::null_mut(),
                None,
                &mut scalars,
                &mut state_flags,
            )
        };
        unsafe {
            let mut unpack: c_int = 1;
            let mut strs = [std::ptr::null(); 3];
            assert_eq!(
                nzbget_rs_options_query(
                    handle,
                    0,
                    c"films-1080p".as_ptr(),
                    1,
                    &mut unpack,
                    strs.as_mut_ptr(),
                    0,
                    None,
                ),
                1
            );
            assert_eq!(unpack, 0);
            assert_eq!(CStr::from_ptr(strs[0]), c"Movies");
            assert_eq!(CStr::from_ptr(strs[1]), c"/dl/movies");
            assert_eq!(
                nzbget_rs_options_query(
                    handle,
                    0,
                    c"films-1080p".as_ptr(),
                    0,
                    &mut unpack,
                    strs.as_mut_ptr(),
                    0,
                    None,
                ),
                0
            );

            unsafe extern "C" fn collect_entry(
                ctx: usize,
                name: *const c_char,
                val: *const c_char,
                restr: c_int,
            ) {
                let v = &mut *(ctx as *mut Vec<(String, String, i32)>);
                v.push((
                    CStr::from_ptr(name).to_str().unwrap().to_owned(),
                    CStr::from_ptr(val).to_str().unwrap().to_owned(),
                    restr,
                ));
            }
            let mut entries: Vec<(String, String, i32)> = Vec::new();
            let count = nzbget_rs_options_query(
                handle,
                1,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                (&mut entries as *mut Vec<_>) as usize,
                Some(collect_entry),
            );
            assert_eq!(count as usize, entries.len());
            assert!(entries
                .iter()
                .any(|(n, v, r)| n == "Server1.Password" && v == "secret" && *r == 1));

            nzbget_rs_options_free(handle);
        }
    }
}
