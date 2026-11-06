//! Options' value parsers (Options.cpp): ParseTime and ParseWeekDays (the
//! scheduler's task times), ValidateOptionName's classification of option
//! names, ConvertOldOption's renames, HasScript and ParseCategorySource, and
//! OptEntry::Restricted's protected option names.
//! Names are compared with the C library's strcasecmp/strncasecmp and numbers
//! read with atoi/strtol, in the current locale, as the C++ did.

use std::ffi::{c_char, c_int, c_long, CStr, CString};

extern "C" {
    fn atoi(s: *const c_char) -> c_int;
    fn strtol(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_long;
    fn strcasecmp(a: *const c_char, b: *const c_char) -> c_int;
    fn strncasecmp(a: *const c_char, b: *const c_char, n: usize) -> c_int;
    fn tolower(c: c_int) -> c_int;
}

fn eq(a: &CStr, b: &CStr) -> bool {
    unsafe { strcasecmp(a.as_ptr(), b.as_ptr()) == 0 }
}

fn eq_n(a: &CStr, b: &CStr, n: usize) -> bool {
    unsafe { strncasecmp(a.as_ptr(), b.as_ptr(), n) == 0 }
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
pub(crate) fn parse_time_with(time: &CStr, mut hours: impl FnMut(i32), mut minutes: impl FnMut(i32)) -> bool {
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
        let h = unsafe { atoi(time.as_ptr()) };
        hours(h);
        if !(0..=23).contains(&h) {
            return false;
        }
    }
    if t.get(colon + 1) == Some(&b'*') {
        return false;
    }
    let m = unsafe { atoi(from(time, colon + 1).as_ptr()) };
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
    /// an obsolete script option: an error when it has a value
    ObsoleteScript = 3,
    /// an obsolete log option (use WriteLog): a warning
    ObsoleteLog = 4,
}

/// The numbered sections and their option suffixes.
const SECTIONS: [(&CStr, usize, &[&CStr]); 4] = [
    (c"server", 6, &[
        c".active", c".name", c".level", c".host", c".port", c".username", c".password", c".joingroup",
        c".encryption", c".connections", c".cipher", c".group", c".retention", c".optional", c".notes",
        c".ipversion", c".certverification",
    ]),
    (c"task", 4, &[c".time", c".weekdays", c".command", c".param", c".downloadrate", c".process"]),
    (c"category", 8, &[c".name", c".destdir", c".extensions", c".unpack", c".aliases"]),
    (c"feed", 4, &[
        c".name", c".url", c".interval", c".filter", c".backlog", c".pausenzb", c".category",
        c".categorySource", c".priority", c".extensions", c".certverification",
    ]),
];

const OBSOLETE: [&CStr; 22] = [
    c"RetryOnCrcError", c"AllowReProcess", c"LoadPars", c"ThreadLimit", c"PostLogKind", c"NZBLogKind",
    c"ProcessLogKind", c"AppendNzbDir", c"RenameBroken", c"MergeNzb", c"StrictParName", c"ReloadUrlQueue",
    c"ReloadPostQueue", c"ParCleanupQueue", c"DeleteCleanupDisk", c"HistoryCleanupDisk", c"SaveQueue",
    c"ReloadQueue", c"TerminateTimeout", c"AccurateRate", c"CreateBrokenLog", c"BrokenLog",
];

/// Options::ValidateOptionName; `predefined` tells whether the name is one of
/// the options (GetOption), asked only after the read-only check.
pub fn validate_option_name(name: &CStr, predefined: &mut dyn FnMut() -> bool) -> NameCheck {
    if [c"ConfigFile", c"AppBin", c"AppDir", c"Version"].iter().any(|n| eq(name, n)) {
        return NameCheck::Invalid;
    }
    if predefined() {
        return NameCheck::Valid;
    }
    for (prefix, len, suffixes) in SECTIONS {
        if eq_n(name, prefix, len) {
            let digits = name.to_bytes()[len.min(name.to_bytes().len())..].iter().take_while(|c| c.is_ascii_digit()).count();
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
    if [c"PostProcess", c"NZBProcess", c"NZBAddedProcess"].iter().any(|n| eq(name, n)) {
        return NameCheck::ObsoleteScript;
    }
    if [c"ScanScript", c"QueueScript", c"FeedScript"].iter().any(|n| eq(name, n)) {
        return NameCheck::Valid;
    }
    if [c"CreateLog", c"ResetLog"].iter().any(|n| eq(name, n)) {
        return NameCheck::ObsoleteLog;
    }
    NameCheck::Invalid
}

/// CString::Replace(from, to): every case-sensitive occurrence, searching on
/// after each replacement.
fn replace_all(s: &mut Vec<u8>, from: &[u8], to: &[u8]) {
    let mut pos = 0;
    loop {
        // CString::Find: nothing at or past the end (except at 0)
        if pos != 0 && pos >= s.len() {
            return;
        }
        let Some(found) = s[pos..].windows(from.len()).position(|w| w == from) else { return };
        let at = pos + found;
        s.splice(at..at + from.len(), to.iter().copied());
        pos = at + to.len();
    }
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
    if eq(&option, c"DefScript") || eq(&option, c"PostScript") {
        option = c"Extensions".to_owned();
    }
    // The original stores strlen in an int before checking the suffix.
    let name_len = option.to_bytes().len() as c_int;
    if eq_n(&option, c"Category", 8)
        && ((name_len > 10 && eq(from(&option, (name_len - 10) as usize), c".DefScript"))
            || (name_len > 11 && eq(from(&option, (name_len - 11) as usize), c".PostScript")))
    {
        let mut v = option.as_bytes().to_vec();
        replace_all(&mut v, b".DefScript", b".Extensions");
        replace_all(&mut v, b".PostScript", b".Extensions");
        option = cstring(v);
    }
    if eq_n(&option, c"Feed", 4) && name_len > 11 && eq(from(&option, (name_len - 11) as usize), c".FeedScript") {
        let mut v = option.as_bytes().to_vec();
        replace_all(&mut v, b".FeedScript", b".Extensions");
        option = cstring(v);
    }
    if eq(&option, c"WriteBufferSize") {
        option = c"WriteBuffer".to_owned();
        // strtol's long cut to an int, as the C++ stored it
        let val = unsafe { strtol(value.as_ptr(), std::ptr::null_mut(), 10) } as c_int;
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

/// Options::HasScript: `name` is in the `,;`-separated list (as Tokenizer
/// splits and trims it), compared with strcasecmp.
pub fn has_script(list: &[u8], name: &CStr) -> bool {
    let list = &list[..tokenizer_length(list.len())];
    crate::util::tokens(list, b",;").any(|t| eq(&cstring(t.to_vec()), name))
}

fn tokenizer_length(len: usize) -> usize {
    // Tokenizer narrows strlen to int to select its BString<1024>. A long
    // input whose narrowed length is small/negative takes that path and is
    // truncated by BString::Set to 1023 bytes. The other path's CString::Set
    // also narrows strlen before copying the string.
    if (len as c_int) < 1023 { len.min(1023) } else { len as c_int as usize }
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
    c"ControlIP", c"ControlPort", c"FormAuth", c"JsonP", c"SecureControl", c"SecurePort", c"SecureCert",
    c"SecureKey", c"CertStore", c"CertCheck", c"AuthorizedIP", c"DaemonUsername", c"UMask",
];

/// Options::OptEntry::Restricted: whether an option's value is protected (shown
/// as "***" to the restricted user): the options above, extension options (a
/// ':' in the name), and names holding "username", "password" or "apikey".
/// Those are searched in the first 1023 bytes (BString<1024>), lowercased with
/// the C library's tolower on each char, as the C++ did. signed_char describes
/// the C++ caller's plain char, which compiler flags can change independently
/// of Rust's c_char target default.
pub fn option_restricted(name: &CStr, signed_char: bool) -> bool {
    let bytes = name.to_bytes();
    // Leave locale-dependent matching to the same C function as the C++.
    if PROTECTED.iter().any(|known| eq(name, known)) || bytes.contains(&b':') {
        return true;
    }
    let mut buf = [0u8; 1023];
    let lower = &mut buf[..bytes.len().min(1023)];
    for (l, &b) in lower.iter_mut().zip(bytes) {
        let c = if signed_char { b as i8 as c_int } else { b as c_int };
        *l = unsafe { tolower(c) } as u8;
    }
    [&b"username"[..], b"password", b"apikey"].iter()
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
    let mut endptr: *mut c_char = std::ptr::null_mut();
    let parsed = unsafe { strtol(val.as_ptr(), &mut endptr, base) } as i32;
    if !endptr.is_null() && unsafe { *endptr } != 0 {
        let fallback = unsafe { strtol(def_val.as_ptr(), std::ptr::null_mut(), base) } as i32;
        (false, fallback)
    } else {
        (true, parsed)
    }
}

/// Options::MergeOldScriptOption category extension option name matcher (`category<digits>.extensions`).
pub fn is_category_extensions_opt(optname: &CStr) -> bool {
    if !eq_n(optname, c"category", 8) {
        return false;
    }
    let bytes = optname.to_bytes();
    let mut idx = 8;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    eq(from(optname, idx), c".extensions")
}

/// Merges `,;`-separated script names from `add_list` into `existing` if not already
/// present (case-insensitively via `has_script`), matching `Options::MergeOldScriptOption`.
pub fn merge_script_list(existing: &[u8], add_list: &[u8]) -> Vec<u8> {
    let mut out = existing.to_vec();
    for script_name in crate::util::tokens(add_list, b",;") {
        let mut cbuf = Vec::with_capacity(script_name.len() + 1);
        cbuf.extend_from_slice(script_name);
        cbuf.push(0);
        if let Ok(cstr) = CStr::from_bytes_with_nul(&cbuf) {
            if !has_script(&out, cstr) {
                if !out.is_empty() {
                    out.push(b',');
                }
                out.extend_from_slice(script_name);
            }
        }
    }
    out
}

/// `Options::ConvertOldOptions`: merges legacy `ScanScript`, `QueueScript`, and `FeedScript`
/// entries into `Extensions` (and `CategoryN.Extensions` when applicable).
pub fn convert_old_options(entries: &mut Vec<(CString, CString)>) {
    for (optname, merge_categories) in [
        (c"ScanScript", true),
        (c"QueueScript", true),
        (c"FeedScript", false),
    ] {
        let Some(opt_idx) = entries.iter().position(|(n, _)| eq(n, optname)) else {
            continue;
        };
        let script_list = entries[opt_idx].1.to_bytes().to_vec();
        if script_list.is_empty() {
            continue;
        }
        let ext_idx = match entries.iter().position(|(n, _)| eq(n, c"Extensions")) {
            Some(idx) => idx,
            None => {
                entries.push((c"Extensions".to_owned(), c"".to_owned()));
                entries.len() - 1
            }
        };
        let merged = merge_script_list(entries[ext_idx].1.to_bytes(), &script_list);
        entries[ext_idx].1 = cstring(merged);
        if merge_categories {
            for (name, val) in entries.iter_mut() {
                if is_category_extensions_opt(name) && !val.to_bytes().is_empty() {
                    let cat_merged = merge_script_list(val.to_bytes(), &script_list);
                    *val = cstring(cat_merged);
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_length_narrowing() {
        assert_eq!(tokenizer_length(0), 0);
        assert_eq!(tokenizer_length(1022), 1022);
        assert_eq!(tokenizer_length(1023), 1023);
        assert_eq!(tokenizer_length(1024), 1024);
        assert_eq!(tokenizer_length(c_int::MAX as usize), c_int::MAX as usize);
        assert_eq!(tokenizer_length(c_int::MAX as usize + 1), 1023);
        if usize::BITS > 32 {
            assert_eq!(tokenizer_length(0x1_0000_0000_u64 as usize), 1023);
            assert_eq!(tokenizer_length(0x1_0000_0400_u64 as usize), 1024);
        }
    }

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
        assert_eq!(validate_option_name(c"Server12.Host", &mut no), NameCheck::Valid);
        assert_eq!(validate_option_name(c"server.port", &mut no), NameCheck::Valid);
        assert_eq!(validate_option_name(c"Feed3.CategorySource", &mut no), NameCheck::Valid);
        assert_eq!(validate_option_name(c"AppDir", &mut || true), NameCheck::Invalid);
        assert_eq!(validate_option_name(c"Script.sh:Opt", &mut no), NameCheck::Valid);
        assert_eq!(validate_option_name(c"loadpars", &mut no), NameCheck::Obsolete);
        assert_eq!(validate_option_name(c"Bogus", &mut no), NameCheck::Invalid);
    }

    #[test]
    fn old_options() {
        let (o, v) = convert_old_option(c"Category2.DefScript", c"x");
        assert_eq!((o.as_bytes(), v.as_bytes()), (b"Category2.Extensions".as_slice(), b"x".as_slice()));
        // matched without case, replaced with case: left as it was
        assert_eq!(convert_old_option(c"category2.defscript", c"x").0.as_bytes(), b"category2.defscript");
        let (o, v) = convert_old_option(c"WriteBufferSize", c"-1");
        assert_eq!((o.as_bytes(), v.as_bytes()), (b"WriteBuffer".as_slice(), b"1024".as_slice()));
        assert_eq!(convert_old_option(c"Decode", c"NO").1.as_bytes(), b"yes");
        assert!(has_script(b"a.py, B.sh", c"b.SH"));
        assert_eq!(parse_category_source(Some(c"FeedFile")), 2);
    }

    #[test]
    fn restricted_options() {
        for name in [c"ControlPassword", c"Server1.Username", c"server2.PASSWORD", c"DupeSearchApiKey", c"JsonP",
            c"jsonp", c"UMASK", c"AuthorizedIP", c"Ext:Option", c":"] {
            assert!(option_restricted(name, c_char::MIN < 0), "{name:?}");
        }
        for name in [c"DupeSearchUrl", c"DupeCheck", c"ControlIPs", c"JsonPx", c"Server1.Host", c""] {
            assert!(!option_restricted(name, c_char::MIN < 0), "{name:?}");
        }
        // only the first 1023 bytes are searched for the words; a ':' anywhere counts
        let mut long = vec![b'a'; 1020];
        long.extend_from_slice(b"password");
        assert!(!option_restricted(&CString::new(long.clone()).unwrap(), c_char::MIN < 0));
        long.truncate(1015);
        long.extend_from_slice(b"password");
        assert!(option_restricted(&CString::new(long.clone()).unwrap(), c_char::MIN < 0));
        long = vec![b'b'; 3023];
        assert!(!option_restricted(&CString::new(long.clone()).unwrap(), c_char::MIN < 0));
        long.push(b':');
        assert!(option_restricted(&CString::new(long).unwrap(), c_char::MIN < 0));
    }

    #[test]
    fn enum_and_int_and_cat_extensions() {
        let names = [c"yes", c"no", c"true", c"false"];
        let vals = [1, 0, 1, 0];
        assert_eq!(parse_enum_value(c"TRUE", c"no", &names, &vals), (true, 1, Some(0)));
        assert_eq!(parse_enum_value(c"yes", c"no", &names, &vals), (true, 1, None));
        assert_eq!(parse_enum_value(c"bogus", c"no", &names, &vals), (false, 0, Some(1)));

        assert_eq!(parse_int_value(c"42", c"10", 10), (true, 42));
        assert_eq!(parse_int_value(c"42abc", c"10", 10), (false, 10));

        assert!(is_category_extensions_opt(c"Category12.Extensions"));
        assert!(is_category_extensions_opt(c"category.extensions"));
        assert!(!is_category_extensions_opt(c"Category12.Name"));
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
    let skip =
        trimmed.is_empty() || trimmed[0] == b'#' || trimmed.iter().all(|&b| b == b' ');
    (len, skip)
}

const SCHEDULER_COMMAND_MAP: [(&[u8], i32); 27] = [
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
    (b"script", 5),
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

