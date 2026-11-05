//! End-to-end Rust implementation of `NzbFile` (`daemon/queue/NzbFile.cpp`):
//! XML SAX state machine (`NzbFileParser`), segment ordering and declared-tail
//! detection (`declared_parts`, `declared_tail`, `file_totals`), duplicate
//! filename detection (`has_duplicate_filenames`), subject deobfuscation and
//! filename validation (`BuildFilenames`), per-file statistics accumulation,
//! content-hash calculation (`calc_nzb_hashes`), and `{{password}}` / metadata
//! extraction and sanitization (`ProcessFiles`).

use std::collections::HashMap;
use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn isspace(c: c_int) -> c_int;
    #[cfg_attr(windows, link_name = "_stricmp")]
    fn strcasecmp(s1: *const c_char, s2: *const c_char) -> c_int;
}

fn cstring_lossy(bytes: &[u8]) -> CString {
    if let Some(nul) = bytes.iter().position(|&b| b == 0) {
        CString::new(&bytes[..nul]).unwrap_or_default()
    } else {
        CString::new(bytes).unwrap_or_default()
    }
}

fn truncate_at_nul(bytes: &[u8]) -> Vec<u8> {
    if let Some(nul) = bytes.iter().position(|&b| b == 0) {
        bytes[..nul].to_vec()
    } else {
        bytes.to_vec()
    }
}

fn text_right_space(byte: u8) -> bool {
    let ch = (byte as c_char) as c_int;
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe { isspace(ch) != 0 }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        if ch < 0 {
            false
        } else {
            unsafe { isspace(ch) != 0 }
        }
    }
}

/// The C library's character classes and lower case under the daemon's
/// locale, as `isspace`/`isdigit((unsigned char)c)` and `tolower(char)` saw them.
pub struct Ctype<'a> {
    /// The classes of unsigned bytes 0..=255.
    pub classes: &'a [u16],
    pub space: u16,
    pub digit: u16,
    /// `tolower` of the char values -128..=255 (index + 128).
    pub lower: &'a [i32],
    /// Whether `char` is signed: `tolower(*p)` took the byte as a `char`.
    pub char_signed: bool,
}

impl Ctype<'_> {
    fn is_space(&self, b: u8) -> bool {
        self.classes[b as usize] & self.space != 0
    }

    fn is_digit(&self, b: u8) -> bool {
        self.classes[b as usize] & self.digit != 0
    }

    /// The byte as the C++ `char`'s value.
    fn char_value(&self, b: u8) -> i32 {
        if self.char_signed {
            b as i8 as i32
        } else {
            b as i32
        }
    }

    /// `*p = tolower(*p)` on a `char`.
    fn to_lower(&self, b: u8) -> u8 {
        self.lower[(self.char_value(b) + 128) as usize] as u8
    }
}

/// `NzbFile::DeclaredParts`: the part count posters put at the very end of
/// the subject, `"(1/N)"` (`N` up to 1,000,000; 0 for none).
pub fn declared_parts(subject: &[u8], ct: &Ctype) -> i32 {
    let s = subject;
    let mut end = s.len();
    while end > 0 && ct.is_space(s[end - 1]) {
        end -= 1;
    }
    if end == 0 || s[end - 1] != b')' {
        return 0;
    }
    let mut p = end - 1;
    let mut total: i64 = 0;
    let mut scale: i64 = 1;
    while p > 0 && ct.is_digit(s[p - 1]) && scale <= 1_000_000 {
        p -= 1;
        total += (ct.char_value(s[p]) - b'0' as i32) as i64 * scale;
        scale *= 10;
    }
    if scale == 1 || p == 0 || s[p - 1] != b'/' {
        return 0;
    }
    p -= 1;
    let digits = p;
    while p > 0 && ct.is_digit(s[p - 1]) {
        p -= 1;
    }
    if p == digits || p == 0 || s[p - 1] != b'(' {
        return 0;
    }
    if total <= 1_000_000 {
        total as i32
    } else {
        0
    }
}

/// `NzbFile::DeclaredTail`: the parts the subject declares when they count
/// past the last listed segment (`last`), else 0. `sizes` are the listed
/// segments' sizes in part order.
pub fn declared_tail(subject: &[u8], last: i32, sizes: &[i64], ct: &Ctype) -> i32 {
    let listed = sizes.len() as i32;
    let declared = declared_parts(subject, ct);
    if listed < 2
        || declared.wrapping_sub(last) < 100
        || (listed as i64) * 3 >= (declared as i64) * 2
    {
        return 0;
    }
    // BString<1024>: the first 1023 bytes, in lower case
    let cut = &subject[..subject.len().min(1023)];
    let lower: Vec<u8> = cut.iter().map(|&b| ct.to_lower(b)).collect();
    // strstr stops at a NUL tolower made (none for real tables)
    let lower = &lower[..lower.iter().position(|&b| b == 0).unwrap_or(lower.len())];
    if crate::bytes::find(lower, b".par2").is_some() {
        return 0;
    }
    let mut sorted = sizes.to_vec();
    let half = sorted.len() / 2;
    let median = *sorted.select_nth_unstable(half).1;
    let last_size = sizes[sizes.len() - 1];
    let same_size = sizes.iter().filter(|&&s| s == median).count() as i64;
    let full = if same_size * 2 >= listed as i64 {
        last_size == median
    } else {
        last_size.wrapping_mul(100) >= median.wrapping_mul(99)
    };
    if median > 0 && full {
        declared
    } else {
        0
    }
}

/// What `NzbFile::AddFileInfo` makes of a file's segments.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct FileTotals {
    /// The segments kept, in part-number order (indexes into the input):
    /// of two with one number the later counts.
    pub order: Vec<usize>,
    pub total_articles: i32,
    pub missed_articles: i32,
    pub size: i64,
    pub remaining_size: i64,
    pub missed_size: i64,
    /// The subject's declared parts counted past the last listed one.
    pub cut_short: bool,
}

/// `NzbFile::AddFileInfo`'s arithmetic over a file's segments (`parts` and
/// `sizes` in the nzb's order). `None` when no segment is left.
pub fn file_totals(
    subject: &[u8],
    parts: &[i32],
    sizes: &[i64],
    ct: &Ctype,
) -> Option<FileTotals> {
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by_key(|&i| parts[i]);
    let mut kept = Vec::with_capacity(order.len());
    for (k, &i) in order.iter().enumerate() {
        if order.get(k + 1).is_none_or(|&next| parts[next] != parts[i]) {
            kept.push(i);
        }
    }
    let first = *kept.first()?;
    let kept_sizes: Vec<i64> = kept.iter().map(|&i| sizes[i]).collect();
    let mut total = parts[kept[kept.len() - 1]];
    let mut cut_short = false;
    let declared = declared_tail(subject, total, &kept_sizes, ct);
    if declared != 0 {
        total = declared;
        cut_short = true;
    }
    let missed = total.wrapping_sub(kept.len() as i32);
    let one = sizes[first];
    let listed: i64 = kept_sizes.iter().fold(0i64, |a, &s| a.wrapping_add(s));
    let missed_size = (missed as i64).wrapping_mul(one);
    let size = listed.wrapping_add(missed_size);
    Some(FileTotals {
        order: kept,
        total_articles: total,
        missed_articles: missed,
        size,
        remaining_size: size.wrapping_sub(missed_size),
        missed_size,
        cut_short,
    })
}

/// `NzbFile::HasDuplicateFilenames`: whether some file's name comes again in
/// two or more later files with other subjects (or in one, when the nzb has
/// just two files), names and subjects compared byte for byte as `strcmp` did.
pub fn has_duplicate_filenames(files: &[(&[u8], &[u8])]) -> bool {
    let mut names: HashMap<&[u8], usize> = HashMap::with_capacity(files.len());
    let mut pairs: HashMap<(&[u8], &[u8]), usize> = HashMap::with_capacity(files.len());
    for &(name, subject) in files.iter().rev() {
        let later = names.get(name).copied().unwrap_or(0);
        let same = pairs.get(&(name, subject)).copied().unwrap_or(0);
        let dupe = 1 + later - same;
        if dupe > 2 || (dupe == 2 && files.len() == 2) {
            return true;
        }
        *names.entry(name).or_default() += 1;
        *pairs.entry((name, subject)).or_default() += 1;
    }
    false
}

/// Extracts `{{password}}` from `filename` in `NzbFile::ReadPasswordFromFilename`.
pub fn extract_password_from_filename(filename: &[u8]) -> Option<&[u8]> {
    let start_pos = filename.windows(2).position(|w| w == b"{{")?;
    let content_start = start_pos + 2;
    let rel_end = filename[content_start..]
        .windows(2)
        .position(|w| w == b"}}")?;
    if rel_end > 0 {
        Some(&filename[content_start..content_start + rel_end])
    } else {
        None
    }
}

/// Formats and sanitizes a segment Message-ID `<tag_content>` in `NzbFile::Parse_EndElement`,
/// replacing bytes `<= b' '` or `== 0x7f` with `b'_'` and truncating the formatted `<%s>`
/// string to 1023 bytes (`BString<1024>`).
pub fn sanitize_segment_message_id(tag_content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(tag_content.len().min(1022) + 2);
    out.push(b'<');
    for &b in tag_content {
        if out.len() >= 1023 {
            break;
        }
        if b <= b' ' || b == 0x7f {
            out.push(b'_');
        } else {
            out.push(b);
        }
    }
    if out.len() < 1023 {
        out.push(b'>');
    }
    out
}

/// Computes `NzbFile::CalcHashes` (`full_content_hash`, `filtered_content_hash`)
/// from files already ordered in `NzbFile::CalcHashes` order.
pub fn calc_nzb_hashes(
    skip_flags: &[u8],
    article_counts: &[usize],
    message_ids: &[&[u8]],
) -> (u32, u32) {
    let file_count = skip_flags.len().min(article_counts.len());
    if file_count == 0 {
        return (0, 0);
    }

    let mut full_hash: u32 = 0;
    let mut filtered_hash: u32 = 0;
    let mut use_for_filtered_count: i32 = 0;
    let mut cur_offset = 0usize;

    for f_idx in 0..file_count {
        let skip = skip_flags[f_idx] != 0;
        let cnt = article_counts[f_idx];
        let end = cur_offset.saturating_add(cnt).min(message_ids.len());
        for &msg_id in &message_ids[cur_offset..end] {
            full_hash = crate::util::hash_bj96(msg_id, full_hash);
            if !skip {
                filtered_hash = crate::util::hash_bj96(msg_id, filtered_hash);
                use_for_filtered_count += 1;
            }
        }
        cur_offset = end;
    }

    if use_for_filtered_count < (file_count as i32) / 2 {
        filtered_hash = 0;
    }

    (full_hash, filtered_hash)
}

/// Detects whether `filename` contains `.par2` (case-insensitive) and accumulates per-file counters into `NzbInfo`
/// during `NzbFile::ProcessFiles`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn accumulate_file_stats(
    filename: &[u8],
    file_total_articles: i32,
    file_missed_articles: i32,
    file_size: i64,
    file_remaining_size: i64,
    file_missed_size: i64,
    nzb_file_count: &mut i32,
    nzb_total_articles: &mut i32,
    nzb_failed_articles: &mut i32,
    nzb_cur_failed_articles: &mut i32,
    nzb_size: &mut i64,
    nzb_remaining_size: &mut i64,
    nzb_failed_size: &mut i64,
    nzb_cur_failed_size: &mut i64,
    nzb_par_size: &mut i64,
    nzb_par_failed_size: &mut i64,
    nzb_par_cur_failed_size: &mut i64,
    nzb_remaining_par_count: &mut i32,
) -> bool {
    let is_par_file = filename
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b".par2"));

    *nzb_file_count = nzb_file_count.wrapping_add(1);
    *nzb_total_articles = nzb_total_articles.wrapping_add(file_total_articles);
    *nzb_failed_articles = nzb_failed_articles.wrapping_add(file_missed_articles);
    *nzb_cur_failed_articles = nzb_cur_failed_articles.wrapping_add(file_missed_articles);
    *nzb_size = nzb_size.wrapping_add(file_size);
    *nzb_remaining_size = nzb_remaining_size.wrapping_add(file_remaining_size);
    *nzb_failed_size = nzb_failed_size.wrapping_add(file_missed_size);
    *nzb_cur_failed_size = *nzb_failed_size;

    if is_par_file {
        *nzb_par_size = nzb_par_size.wrapping_add(file_size);
        *nzb_par_failed_size = nzb_par_failed_size.wrapping_add(file_missed_size);
        *nzb_par_cur_failed_size = *nzb_par_failed_size;
        *nzb_remaining_par_count = nzb_remaining_par_count.wrapping_add(1);
    }

    is_par_file
}

fn classify_meta_type(attr_name: &CStr, attr_val: &CStr) -> i32 {
    unsafe {
        if strcasecmp(attr_name.as_ptr(), c"type".as_ptr()) != 0 {
            return 0;
        }
        if strcasecmp(attr_val.as_ptr(), c"password".as_ptr()) == 0 {
            1
        } else if strcasecmp(attr_val.as_ptr(), c"category".as_ptr()) == 0 {
            2
        } else if strcasecmp(attr_val.as_ptr(), c"name".as_ptr()) == 0 {
            3
        } else if strcasecmp(attr_val.as_ptr(), c"title".as_ptr()) == 0 {
            4
        } else {
            0
        }
    }
}

#[derive(Clone, Debug)]
struct PendingNzbArticle {
    part_number: i32,
    size: i64,
    message_id: CString,
}

#[derive(Clone, Debug)]
struct PendingNzbFile {
    id: i32,
    subject: Option<CString>,
    time: i32,
    groups: Vec<CString>,
    articles: Vec<PendingNzbArticle>,
}

#[derive(Clone, Debug)]
pub struct ParsedNzbMessage {
    pub kind: i32, // 1 = Warning, 2 = Error
    pub text: CString,
}

#[derive(Clone, Debug)]
pub struct ParsedNzbFile {
    pub id: i32,
    pub subject: CString,
    pub filename: CString,
    pub time: i32,
    pub size: i64,
    pub remaining_size: i64,
    pub missed_size: i64,
    pub total_articles: i32,
    pub missed_articles: i32,
    pub par_file: bool,
    pub cut_short: bool,
    pub groups: Vec<CString>,
    pub group_ptrs: Vec<*const c_char>,
    pub article_parts: Vec<i32>,
    pub article_sizes: Vec<i64>,
    pub article_msg_ids: Vec<CString>,
    pub article_msg_id_ptrs: Vec<*const c_char>,
}

pub struct NzbFileParser {
    file_name: CString,
    pub category: CString,
    pub password: CString,
    pub meta_name: CString,
    pub meta_title: CString,
    pub many_dupe_files: bool,
    pub file_count: i32,
    pub total_articles: i32,
    pub failed_articles: i32,
    pub cur_failed_articles: i32,
    pub size: i64,
    pub remaining_size: i64,
    pub failed_size: i64,
    pub cur_failed_size: i64,
    pub par_size: i64,
    pub par_failed_size: i64,
    pub par_cur_failed_size: i64,
    pub remaining_par_count: i32,
    pub full_content_hash: u32,
    pub filtered_content_hash: u32,
    pub messages: Vec<ParsedNzbMessage>,
    pub files: Vec<ParsedNzbFile>,
    closed_files: Vec<PendingNzbFile>,
    current_file: Option<PendingNzbFile>,
    current_article_idx: Option<usize>,
    tag_content: Vec<u8>,
    ignore_next_error: bool,
    has_password: bool,
    has_category: bool,
    has_name: bool,
    has_title: bool,
    current_element: Vec<u8>,
}

impl NzbFileParser {
    pub fn new(file_name: &[u8]) -> Self {
        Self {
            file_name: cstring_lossy(file_name),
            category: CString::default(),
            password: CString::default(),
            meta_name: CString::default(),
            meta_title: CString::default(),
            many_dupe_files: false,
            file_count: 0,
            total_articles: 0,
            failed_articles: 0,
            cur_failed_articles: 0,
            size: 0,
            remaining_size: 0,
            failed_size: 0,
            cur_failed_size: 0,
            par_size: 0,
            par_failed_size: 0,
            par_cur_failed_size: 0,
            remaining_par_count: 0,
            full_content_hash: 0,
            filtered_content_hash: 0,
            messages: Vec::new(),
            files: Vec::new(),
            closed_files: Vec::new(),
            current_file: None,
            current_article_idx: None,
            tag_content: Vec::new(),
            ignore_next_error: false,
            has_password: false,
            has_category: false,
            has_name: false,
            has_title: false,
            current_element: Vec::new(),
        }
    }

    fn push_warning(&mut self, msg: Vec<u8>) {
        self.messages.push(ParsedNzbMessage {
            kind: 1,
            text: cstring_lossy(&msg),
        });
    }

    fn push_error(&mut self, msg: Vec<u8>) {
        self.messages.push(ParsedNzbMessage {
            kind: 2,
            text: cstring_lossy(&msg),
        });
    }

    fn format_missing_attrs_msg(tag_name: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(48 + tag_name.len());
        out.extend_from_slice(b"Malformed nzb-file, tag <");
        out.extend_from_slice(tag_name);
        out.extend_from_slice(b"> without attributes");
        out
    }

    pub fn start_element(
        &mut self,
        name: &[u8],
        has_atts: bool,
        attrs: &[(&[u8], Option<&[u8]>)],
        alloc_file_id: impl FnOnce() -> i32,
    ) {
        self.current_element.clear();
        self.current_element.extend_from_slice(name);
        self.tag_content.clear();

        if name == b"file" {
            let id = alloc_file_id();
            self.current_file = Some(PendingNzbFile {
                id,
                subject: None,
                time: 0,
                groups: Vec::new(),
                articles: Vec::new(),
            });
            self.current_article_idx = None;

            if !has_atts {
                let msg = Self::format_missing_attrs_msg(name);
                self.push_warning(msg);
                return;
            }

            if let Some(file) = self.current_file.as_mut() {
                for &(attr_name, attr_val) in attrs {
                    if attr_name == b"subject" {
                        file.subject = attr_val.map(cstring_lossy);
                    } else if attr_name == b"date" {
                        file.time = crate::feed::c_atoi_bytes(attr_val.unwrap_or(b""));
                    }
                }
            }
        } else if name == b"segment" {
            if self.current_file.is_none() {
                self.push_warning(b"Malformed nzb-file, tag <segment> without tag <file>".to_vec());
                return;
            }
            if !has_atts {
                let msg = Self::format_missing_attrs_msg(name);
                self.push_warning(msg);
                return;
            }

            let mut lsize: i64 = -1;
            let mut part_number: i32 = -1;
            for &(attr_name, attr_val) in attrs {
                let val = attr_val.unwrap_or(b"");
                if attr_name == b"bytes" {
                    lsize = crate::feed::c_atoll_bytes(val);
                } else if attr_name == b"number" {
                    part_number = crate::feed::c_atoi_bytes(val);
                }
            }

            if part_number > 0 {
                if let Some(file) = self.current_file.as_mut() {
                    file.articles.push(PendingNzbArticle {
                        part_number,
                        size: i64::from(lsize as i32),
                        message_id: CString::default(),
                    });
                    self.current_article_idx = Some(file.articles.len() - 1);
                }
            }
        } else if name == b"meta" {
            self.has_password = false;
            self.has_category = false;
            self.has_name = false;
            self.has_title = false;

            if !has_atts {
                let msg = Self::format_missing_attrs_msg(name);
                self.push_warning(msg);
                return;
            }

            for &(attr_name, attr_val) in attrs {
                let Some(val) = attr_val else {
                    break;
                };
                let n_cstr = cstring_lossy(attr_name);
                let v_cstr = cstring_lossy(val);
                match classify_meta_type(&n_cstr, &v_cstr) {
                    1 => self.has_password = true,
                    2 => self.has_category = true,
                    3 => self.has_name = true,
                    4 => self.has_title = true,
                    _ => {}
                }
            }
        }
    }

    pub fn end_element(&mut self, name: &[u8]) {
        if name == b"file" {
            if let Some(pending) = self.current_file.take() {
                self.closed_files.push(pending);
            }
            self.current_article_idx = None;
        } else if name == b"group" {
            if let Some(file) = self.current_file.as_mut() {
                file.groups.push(cstring_lossy(&self.tag_content));
                self.tag_content.clear();
            }
        } else if name == b"segment" {
            if let (Some(file), Some(idx)) =
                (self.current_file.as_mut(), self.current_article_idx.take())
            {
                let tag_bytes = truncate_at_nul(&self.tag_content);
                let sanitized = sanitize_segment_message_id(&tag_bytes);
                if let Some(article) = file.articles.get_mut(idx) {
                    article.message_id = cstring_lossy(&sanitized);
                }
            }
        } else if name == b"meta" {
            let content = cstring_lossy(&self.tag_content);
            if self.has_password {
                self.password = content;
            } else if self.has_category {
                self.category = content;
            } else if self.has_name {
                self.meta_name = content;
            } else if self.has_title {
                self.meta_title = content;
            }
        }

        self.current_element.clear();
    }

    pub fn characters(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let should_trim = self.current_element == b"meta" && self.has_category;
        if should_trim {
            let (start, end) = crate::util::trim_string(chunk, true, true, text_right_space);
            if start < end {
                self.tag_content.extend_from_slice(&chunk[start..end]);
            }
        } else {
            self.tag_content.extend_from_slice(chunk);
        }
    }

    pub fn on_missing_entity(&mut self) {
        self.push_warning(b"entity not found".to_vec());
        self.ignore_next_error = true;
    }

    pub fn on_sax_error(&mut self, err_msg: &[u8]) {
        if self.ignore_next_error {
            self.ignore_next_error = false;
            return;
        }
        let msg = truncate_at_nul(err_msg);
        let trimmed_len = crate::twinproberenhist::tpr_nzbfile_trim_sax_error_trailing(&msg);
        let full = crate::twinproberenhist::tpr_nzbfile_format_sax_error_msg(&msg[..trimmed_len]);
        self.push_error(full);
    }

    pub fn finalize(&mut self, sax_ret: i32, par_ignore_ext: &[u8], ct: &Ctype) -> bool {
        self.finalize_with(
            sax_ret,
            par_ignore_ext,
            ct,
            |_, _| {},
            None::<fn(&[*const c_char], &mut [usize])>,
        )
    }

    pub fn finalize_with(
        &mut self,
        sax_ret: i32,
        par_ignore_ext: &[u8],
        ct: &Ctype,
        mut on_parsed_subject: impl FnMut(&CStr, &CStr),
        sort_indices: Option<impl FnOnce(&[*const c_char], &mut [usize])>,
    ) -> bool {
        let file_name_bytes = self.file_name.as_bytes();
        let base_idx = crate::paths::base_file_name(file_name_bytes);
        let base_name = &file_name_bytes[base_idx..];

        if sax_ret != 0 {
            self.push_error(crate::twinproberenhist::tpr_nzbfile_format_parse_error_msg(
                base_name, false,
            ));
            return false;
        }

        // 1. AddFileInfo over closed_files
        let closed = std::mem::take(&mut self.closed_files);
        let mut parsed_files: Vec<(Option<CString>, CString, ParsedNzbFile)> =
            Vec::with_capacity(closed.len());
        for pending in closed {
            let parts: Vec<i32> = pending.articles.iter().map(|a| a.part_number).collect();
            let sizes: Vec<i64> = pending.articles.iter().map(|a| a.size).collect();
            let subj_slice = pending
                .subject
                .as_ref()
                .map(|s| s.as_bytes())
                .unwrap_or(b"");
            let Some(totals) = file_totals(subj_slice, &parts, &sizes, ct) else {
                continue;
            };

            let mut article_parts = Vec::with_capacity(totals.order.len());
            let mut article_sizes = Vec::with_capacity(totals.order.len());
            let mut article_msg_ids = Vec::with_capacity(totals.order.len());
            for &idx in &totals.order {
                let art = &pending.articles[idx];
                article_parts.push(art.part_number);
                article_sizes.push(art.size);
                article_msg_ids.push(art.message_id.clone());
            }
            let article_msg_id_ptrs = article_msg_ids.iter().map(|s| s.as_ptr()).collect();
            let group_ptrs = pending.groups.iter().map(|s| s.as_ptr()).collect();

            parsed_files.push((
                pending.subject,
                self.file_name.clone(),
                ParsedNzbFile {
                    id: pending.id,
                    subject: CString::default(),
                    filename: CString::default(),
                    time: pending.time,
                    size: totals.size,
                    remaining_size: totals.remaining_size,
                    missed_size: totals.missed_size,
                    total_articles: totals.total_articles,
                    missed_articles: totals.missed_articles,
                    par_file: false,
                    cut_short: totals.cut_short,
                    groups: pending.groups,
                    group_ptrs,
                    article_parts,
                    article_sizes,
                    article_msg_ids,
                    article_msg_id_ptrs,
                },
            ));
        }

        if parsed_files.is_empty() {
            self.push_error(crate::twinproberenhist::tpr_nzbfile_format_parse_error_msg(
                base_name, true,
            ));
            return false;
        }

        // 2. BuildFilenames
        // First pass: ParseSubject(fileInfo, true)
        for (opt_subj, cur_filename, file) in &mut parsed_files {
            if let Some(subj) = opt_subj {
                let deobf = crate::deobfuscation::deobfuscate(subj.as_bytes());
                *cur_filename = cstring_lossy(&deobf);
                file.subject = subj.clone();
                on_parsed_subject(&file.subject, cur_filename);
            } else {
                let fallback = cstring_lossy(format!("{}", file.id).as_bytes());
                *opt_subj = Some(fallback.clone());
                file.subject = fallback;
            }
        }

        let check_dupes = |items: &[(Option<CString>, CString, ParsedNzbFile)]| -> bool {
            let pairs: Vec<(&[u8], &[u8])> = items
                .iter()
                .map(|(_, fname, f)| (fname.as_bytes(), f.subject.as_bytes()))
                .collect();
            has_duplicate_filenames(&pairs)
        };

        if check_dupes(&parsed_files) {
            // Second pass: ParseSubject(fileInfo, false)
            for (_, cur_filename, file) in &mut parsed_files {
                let deobf = crate::deobfuscation::deobfuscate(file.subject.as_bytes());
                *cur_filename = cstring_lossy(&deobf);
                on_parsed_subject(&file.subject, cur_filename);
            }
        }

        if check_dupes(&parsed_files) {
            self.many_dupe_files = true;
            for (_, cur_filename, file) in &mut parsed_files {
                *cur_filename = file.subject.clone();
            }
        }

        // 3. ProcessFiles: MakeValidFilename + accumulate_file_stats
        self.files.reserve(parsed_files.len());
        for (_, cur_filename, mut file) in parsed_files {
            let valid_name = crate::paths::make_valid_filename(cur_filename.as_bytes(), false);
            file.filename = cstring_lossy(&valid_name);
            file.par_file = accumulate_file_stats(
                file.filename.as_bytes(),
                file.total_articles,
                file.missed_articles,
                file.size,
                file.remaining_size,
                file.missed_size,
                &mut self.file_count,
                &mut self.total_articles,
                &mut self.failed_articles,
                &mut self.cur_failed_articles,
                &mut self.size,
                &mut self.remaining_size,
                &mut self.failed_size,
                &mut self.cur_failed_size,
                &mut self.par_size,
                &mut self.par_failed_size,
                &mut self.par_cur_failed_size,
                &mut self.remaining_par_count,
            );
            self.files.push(file);
        }

        // 4. CalcHashes
        let mut sorted_indices: Vec<usize> = (0..self.files.len()).collect();
        if let Some(sort_fn) = sort_indices {
            let filename_ptrs: Vec<*const c_char> =
                self.files.iter().map(|f| f.filename.as_ptr()).collect();
            sort_fn(&filename_ptrs, &mut sorted_indices);
        } else {
            sorted_indices.sort_by(|&a, &b| {
                self.files[b]
                    .filename
                    .as_bytes()
                    .cmp(self.files[a].filename.as_bytes())
            });
        }

        let fold = |b: u8| b.to_ascii_lowercase() as std::ffi::c_int;
        let lower = crate::wildmask::Lower::Fold(&fold);
        let mut skip_flags = Vec::with_capacity(self.files.len());
        let mut article_counts = Vec::with_capacity(self.files.len());
        let mut message_ids: Vec<&[u8]> = Vec::new();
        for &idx in &sorted_indices {
            let f = &self.files[idx];
            let skip = !f.par_file
                && crate::util::match_file_ext(
                    f.filename.as_bytes(),
                    par_ignore_ext,
                    b",;",
                    &lower,
                    &lower,
                );
            skip_flags.push(u8::from(skip));
            article_counts.push(f.article_msg_ids.len());
            for mid in &f.article_msg_ids {
                message_ids.push(mid.as_bytes());
            }
        }
        let (full_hash, filtered_hash) =
            calc_nzb_hashes(&skip_flags, &article_counts, &message_ids);
        self.full_content_hash = full_hash;
        self.filtered_content_hash = filtered_hash;

        // 5. Password & meta sanitization
        if self.password.as_bytes().is_empty() {
            if let Some(pw) = extract_password_from_filename(self.file_name.as_bytes()) {
                self.password = cstring_lossy(pw);
            }
        }

        let san_meta_name = crate::paths::sanitize_path_segment(self.meta_name.as_bytes());
        let san_meta_title = crate::paths::sanitize_path_segment(self.meta_title.as_bytes());
        self.meta_name = if san_meta_name.is_empty() && !san_meta_title.is_empty() {
            cstring_lossy(&san_meta_title)
        } else {
            cstring_lossy(&san_meta_name)
        };
        self.meta_title = cstring_lossy(&san_meta_title);

        let mut cat_bytes = self.category.as_bytes().to_vec();
        let (cat_start, cat_end) =
            crate::util::sanitize_line(&mut cat_bytes, text_right_space);
        self.category = cstring_lossy(&cat_bytes[cat_start..cat_end]);

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_c_locale<T>(f: impl FnOnce(&Ctype) -> T) -> T {
        let mut classes = [0u16; 256];
        for (b, slot) in classes.iter_mut().enumerate() {
            if matches!(b as u8, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
                *slot |= 1;
            }
            if (b as u8).is_ascii_digit() {
                *slot |= 2;
            }
        }
        let lower: Vec<i32> = (-128..256)
            .map(|c: i32| if (65..=90).contains(&c) { c + 32 } else { c })
            .collect();
        f(&Ctype {
            classes: &classes,
            space: 1,
            digit: 2,
            lower: &lower,
            char_signed: true,
        })
    }

    #[test]
    fn parts() {
        with_c_locale(|ct| {
            assert_eq!(declared_parts(b"x yEnc (1/3709)", ct), 3709);
            assert_eq!(declared_parts(b"x yEnc (12/3709) \r\n", ct), 3709);
            assert_eq!(declared_parts(b"x (1/1000000)", ct), 1_000_000);
            assert_eq!(declared_parts(b"x (1/1000001)", ct), 0);
            assert_eq!(declared_parts(b"x (1/12345678)", ct), 0);
            assert_eq!(declared_parts(b"x (/37)", ct), 0);
            assert_eq!(declared_parts(b"(1/37)", ct), 37);
            assert_eq!(declared_parts(b"1/37)", ct), 0);
            assert_eq!(declared_parts(b"x [1/37]", ct), 0);
            assert_eq!(declared_parts(b"", ct), 0);
            assert_eq!(declared_parts(b")", ct), 0);
        });
    }

    #[test]
    fn tails() {
        with_c_locale(|ct| {
            let full = vec![740_000i64; 20];
            assert_eq!(declared_tail(b"a.rar yEnc (1/3709)", 20, &full, ct), 3709);
            assert_eq!(declared_tail(b"a.rar yEnc (1/119)", 20, &full, ct), 0);
            assert_eq!(declared_tail(b"a.rar yEnc (1/29)", 20, &full, ct), 0);
            assert_eq!(declared_tail(b"A.PAR2 yEnc (1/3709)", 20, &full, ct), 0);
            let mut short = full.clone();
            *short.last_mut().unwrap() = 1000;
            assert_eq!(declared_tail(b"a.rar yEnc (1/3709)", 20, &short, ct), 0);
            assert_eq!(
                declared_tail(b"a.rar yEnc (1/3709)", 1, &full[..1], ct),
                0
            );
            let mixed: Vec<i64> = (0..20).map(|i| 700_000 + i * 1000).collect();
            assert_eq!(
                declared_tail(b"a.rar yEnc (1/3709)", 20, &mixed, ct),
                3709
            );
            let long = [vec![b'x'; 1020], b".par2 (1/3709)".to_vec()].concat();
            assert_eq!(declared_tail(&long, 20, &full, ct), 3709);
        });
    }

    #[test]
    fn totals() {
        with_c_locale(|ct| {
            let t = file_totals(b"s", &[3, 1, 2, 2, 5], &[30, 10, 20, 21, 50], ct).unwrap();
            assert_eq!(t.order, vec![1, 3, 0, 4]);
            assert_eq!((t.total_articles, t.missed_articles), (5, 1));
            assert_eq!(
                (t.size, t.remaining_size, t.missed_size),
                (10 + 21 + 30 + 50 + 10, 111, 10)
            );
            assert!(!t.cut_short);
            assert_eq!(file_totals(b"s", &[], &[], ct), None);
        });
    }

    #[test]
    fn duplicates() {
        let f = |v: &[(&'static str, &'static str)]| {
            has_duplicate_filenames(
                &v.iter()
                    .map(|&(a, b)| (a.as_bytes(), b.as_bytes()))
                    .collect::<Vec<_>>(),
            )
        };
        assert!(!f(&[("a", "1"), ("b", "2"), ("a", "3")]));
        assert!(f(&[("a", "1"), ("b", "2"), ("a", "3"), ("a", "4")]));
        assert!(f(&[("a", "1"), ("a", "2")]));
        assert!(!f(&[("a", "1"), ("a", "1")]));
        assert!(!f(&[("a", "1"), ("a", "1"), ("a", "1")]));
        assert!(!f(&[("a", "1"), ("A", "2"), ("a", "3")]));
        assert!(f(&[("a", "1"), ("a", "1"), ("a", "2"), ("a", "3")]));
        assert!(!f(&[]));
    }

    #[test]
    fn parser_end_to_end() {
        with_c_locale(|ct| {
            let mut parser = NzbFileParser::new(b"/tmp/test{{pw123}}.nzb");
            parser.start_element(b"meta", true, &[(b"type", Some(b"category"))], || 1);
            parser.characters(b"TV ");
            parser.characters(b">");
            parser.characters(b" 4K");
            parser.end_element(b"meta");

            parser.start_element(b"meta", true, &[(b"type", Some(b"title"))], || 1);
            parser.characters(b"My.Release.S01E01.mkv");
            parser.end_element(b"meta");

            parser.start_element(
                b"file",
                true,
                &[
                    (b"poster", Some(b"p@test")),
                    (b"date", Some(b"1335508618")),
                    (b"subject", Some(b"\"a.mkv\" yEnc (1/2)")),
                ],
                || 10,
            );
            parser.start_element(b"group", true, &[], || 0);
            parser.characters(b"alt.binaries.test");
            parser.end_element(b"group");
            parser.start_element(
                b"segment",
                true,
                &[(b"bytes", Some(b"100")), (b"number", Some(b"2"))],
                || 0,
            );
            parser.characters(b"msg2\r\n@test");
            parser.end_element(b"segment");
            parser.start_element(
                b"segment",
                true,
                &[(b"bytes", Some(b"100")), (b"number", Some(b"1"))],
                || 0,
            );
            parser.characters(b"msg1@test");
            parser.end_element(b"segment");
            parser.end_element(b"file");

            assert!(parser.finalize(0, b".nfo", ct));
            assert_eq!(parser.category.as_bytes(), b"TV>4K");
            assert_eq!(parser.password.as_bytes(), b"pw123");
            assert_eq!(parser.meta_name.as_bytes(), b"My.Release.S01E01.mkv");
            assert_eq!(parser.files.len(), 1);
            assert_eq!(parser.files[0].filename.as_bytes(), b"a.mkv");
            assert_eq!(parser.files[0].article_msg_ids[0].as_bytes(), b"<msg1@test>");
            assert_eq!(
                parser.files[0].article_msg_ids[1].as_bytes(),
                b"<msg2__@test>"
            );
        });
    }
}
