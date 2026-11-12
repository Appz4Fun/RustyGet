//! End-to-end Rust implementation of `NzbFile` (`daemon/queue/NzbFile.cpp`):
//! XML SAX state machine (`NzbFileParser`), segment ordering and declared-tail
//! detection (`declared_parts`, `declared_tail`, `file_totals`), duplicate
//! filename detection (`has_duplicate_filenames`), subject deobfuscation and
//! filename validation (`BuildFilenames`), per-file statistics accumulation,
//! content-hash calculation (`calc_nzb_hashes`), and `{{password}}` / metadata
//! extraction and sanitization (`ProcessFiles`).

use std::collections::HashMap;
use std::ffi::{c_char, c_int, CStr, CString};

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
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
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
pub fn file_totals(subject: &[u8], parts: &[i32], sizes: &[i64], ct: &Ctype) -> Option<FileTotals> {
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
    if !attr_name.to_bytes().eq_ignore_ascii_case(b"type") {
        return 0;
    }
    let val = attr_val.to_bytes();
    if val.eq_ignore_ascii_case(b"password") {
        1
    } else if val.eq_ignore_ascii_case(b"category") {
        2
    } else if val.eq_ignore_ascii_case(b"name") {
        3
    } else if val.eq_ignore_ascii_case(b"title") {
        4
    } else {
        0
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
        let trimmed_len = nzbfile_trim_sax_error_trailing(&msg);
        let full = nzbfile_format_sax_error_msg(&msg[..trimmed_len]);
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
            self.push_error(nzbfile_format_parse_error_msg(base_name, false));
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
            self.push_error(nzbfile_format_parse_error_msg(base_name, true));
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
        let (cat_start, cat_end) = crate::util::sanitize_line(&mut cat_bytes, text_right_space);
        self.category = cstring_lossy(&cat_bytes[cat_start..cat_end]);

        true
    }

    fn normalize_xml_eol(s: &str) -> std::borrow::Cow<'_, str> {
        if !s.as_bytes().contains(&b'\r') {
            return std::borrow::Cow::Borrowed(s);
        }
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            } else {
                out.push(c);
            }
        }
        std::borrow::Cow::Owned(out)
    }

    fn normalize_xml_attr_ws(s: &str) -> std::borrow::Cow<'_, str> {
        if !s.bytes().any(|b| matches!(b, b'\r' | b'\n' | b'\t')) {
            return std::borrow::Cow::Borrowed(s);
        }
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(' ');
            } else if c == '\n' || c == '\t' {
                out.push(' ');
            } else {
                out.push(c);
            }
        }
        std::borrow::Cow::Owned(out)
    }

    fn parse_element_attrs<R>(
        &mut self,
        e: &quick_xml::events::BytesStart<'_>,
        reader: &quick_xml::Reader<R>,
    ) -> Result<(bool, Vec<(Vec<u8>, Vec<u8>)>), ()> {
        let mut has_atts = false;
        let mut attrs = Vec::new();
        for attr_res in e.attributes() {
            has_atts = true;
            let a = match attr_res {
                Ok(a) => a,
                Err(err) => {
                    self.on_sax_error(err.to_string().as_bytes());
                    return Err(());
                }
            };
            let decoded = match reader.decoder().decode(&a.value) {
                Ok(d) => d,
                Err(err) => {
                    self.on_sax_error(err.to_string().as_bytes());
                    return Err(());
                }
            };
            let norm = Self::normalize_xml_attr_ws(&decoded);
            let unescaped = match quick_xml::escape::unescape(&norm) {
                Ok(u) => u,
                Err(err) => {
                    self.on_sax_error(err.to_string().as_bytes());
                    return Err(());
                }
            };
            attrs.push((a.key.as_ref().to_vec(), unescaped.into_owned().into_bytes()));
        }
        Ok((has_atts, attrs))
    }

    /// Streams and parses XML from `input` into `self` (`xmlSAXUserParseFile` replacement),
    /// returning `0` on success or `-1` on XML syntax/entity/encoding error.
    pub fn parse_xml_reader<R: std::io::Read>(
        &mut self,
        input: R,
        mut alloc_file_id: impl FnMut() -> i32,
    ) -> i32 {
        use quick_xml::events::Event;
        let stream = match XmlStreamReader::new(input) {
            Ok(s) => s,
            Err(err) => {
                self.on_sax_error(err.to_string().as_bytes());
                return -1;
            }
        };
        if stream.is_empty() {
            self.on_sax_error(b"Document is empty");
            return -1;
        }
        let mut reader = quick_xml::Reader::from_reader(stream);
        let mut buf = Vec::new();
        let mut saw_root = false;
        let mut root_closed = false;
        let mut depth: i32 = 0;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    if root_closed {
                        self.on_sax_error(b"Extra content at the end of the document");
                        return -1;
                    }
                    saw_root = true;
                    depth += 1;
                    let Ok((has_atts, attrs)) = self.parse_element_attrs(&e, &reader) else {
                        return -1;
                    };
                    let attr_refs: Vec<(&[u8], Option<&[u8]>)> = attrs
                        .iter()
                        .map(|(k, v)| (k.as_slice(), Some(v.as_slice())))
                        .collect();
                    self.start_element(e.name().as_ref(), has_atts, &attr_refs, &mut alloc_file_id);
                }
                Ok(Event::Empty(e)) => {
                    if root_closed {
                        self.on_sax_error(b"Extra content at the end of the document");
                        return -1;
                    }
                    saw_root = true;
                    let Ok((has_atts, attrs)) = self.parse_element_attrs(&e, &reader) else {
                        return -1;
                    };
                    let attr_refs: Vec<(&[u8], Option<&[u8]>)> = attrs
                        .iter()
                        .map(|(k, v)| (k.as_slice(), Some(v.as_slice())))
                        .collect();
                    self.start_element(e.name().as_ref(), has_atts, &attr_refs, &mut alloc_file_id);
                    self.end_element(e.name().as_ref());
                    if depth == 0 {
                        root_closed = true;
                    }
                }
                Ok(Event::Text(t)) => {
                    let Ok(decoded) = t.decode() else {
                        self.on_sax_error(b"Invalid character encoding");
                        return -1;
                    };
                    let norm = Self::normalize_xml_eol(&decoded);
                    if depth == 0 {
                        let rest = norm.strip_prefix('\u{feff}').unwrap_or(&norm);
                        if !rest
                            .bytes()
                            .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                        {
                            self.on_sax_error(b"Extra content at the end of the document");
                            return -1;
                        }
                    } else {
                        self.characters(norm.as_bytes());
                    }
                }
                Ok(Event::CData(c)) => {
                    if depth == 0 {
                        self.on_sax_error(b"CDATA section outside root element");
                        return -1;
                    }
                    let Ok(decoded) = c.decode() else {
                        self.on_sax_error(b"Invalid character encoding");
                        return -1;
                    };
                    let norm = Self::normalize_xml_eol(&decoded);
                    self.characters(norm.as_bytes());
                }
                Ok(Event::GeneralRef(r)) => {
                    if depth == 0 {
                        self.on_sax_error(b"Entity reference outside root element");
                        return -1;
                    }
                    let entity = match r.as_ref() {
                        b"lt" => Some(b"<".to_vec()),
                        b"gt" => Some(b">".to_vec()),
                        b"amp" => Some(b"&".to_vec()),
                        b"quot" => Some(b"\"".to_vec()),
                        b"apos" => Some(b"'".to_vec()),
                        _ => r
                            .resolve_char_ref()
                            .ok()
                            .flatten()
                            .map(|c| c.to_string().into_bytes()),
                    };
                    match entity {
                        Some(bytes) => self.characters(&bytes),
                        None => {
                            self.on_missing_entity();
                            self.on_sax_error(b"Entity not defined");
                            return -1;
                        }
                    }
                }
                Ok(Event::Decl(_)) => {
                    if saw_root {
                        self.on_sax_error(
                            b"XML declaration allowed only at the start of the document",
                        );
                        return -1;
                    }
                }
                Ok(Event::DocType(d)) => {
                    if saw_root || d.windows(8).any(|w| w.eq_ignore_ascii_case(b"<!entity")) {
                        self.on_sax_error(b"Entity declarations are not supported");
                        return -1;
                    }
                }
                Ok(Event::End(e)) => {
                    depth -= 1;
                    if depth < 0 {
                        self.on_sax_error(b"Unexpected closing tag");
                        return -1;
                    }
                    self.end_element(e.name().as_ref());
                    if depth == 0 {
                        root_closed = true;
                    }
                }
                Ok(Event::Eof) => break,
                Err(err) => {
                    self.on_sax_error(err.to_string().as_bytes());
                    return -1;
                }
                _ => {}
            }
            buf.clear();
        }
        if saw_root && root_closed && depth == 0 {
            0
        } else {
            self.on_sax_error(b"Premature end of data");
            -1
        }
    }

    /// Parses raw XML bytes into `self` (`xmlSAXUserParseFile` replacement),
    /// returning `0` on success or `-1` on XML syntax/entity error.
    pub fn parse_xml(&mut self, xml: &[u8], alloc_file_id: impl FnMut() -> i32) -> i32 {
        self.parse_xml_reader(std::io::Cursor::new(xml), alloc_file_id)
    }

    /// Streams an `.nzb` file from disk, runs `parse_xml_reader`, and finalizes it (`NzbFile::Parse`).
    pub fn parse_file(
        &mut self,
        par_ignore_ext: &[u8],
        ct: &Ctype,
        alloc_file_id: impl FnMut() -> i32,
        on_parsed_subject: impl FnMut(&CStr, &CStr),
        sort_indices: Option<impl FnOnce(&[*const c_char], &mut [usize])>,
    ) -> bool {
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            std::path::Path::new(std::ffi::OsStr::from_bytes(self.file_name.as_bytes()))
        };
        #[cfg(not(unix))]
        let path_str = String::from_utf8_lossy(self.file_name.as_bytes()).into_owned();
        #[cfg(not(unix))]
        let path = std::path::Path::new(&path_str);

        let sax_ret = match std::fs::File::open(path) {
            Ok(file) => self.parse_xml_reader(file, alloc_file_id),
            Err(_) => -1,
        };
        self.finalize_with(sax_ret, par_ignore_ext, ct, on_parsed_subject, sort_indices)
    }
}

#[cfg(unix)]
pub(crate) struct ParsedMaterializedFile {
    pub file_ptr: usize,
    pub total_articles: i32,
    pub missed_articles: i32,
    pub cut_short: bool,
}

#[cfg(unix)]
pub(crate) struct ParsedMaterializedNzb {
    pub ok: bool,
    pub uptr_addr: usize,
    pub nzb_ptr: usize,
    pub password: Vec<u8>,
    pub category_from_file: Vec<u8>,
    pub files: Vec<ParsedMaterializedFile>,
}

#[cfg(unix)]
fn with_libc_ctype<R>(f: impl FnOnce(Ctype<'_>) -> R) -> R {
    let mut classes = [0u16; 256];
    let mut lower = [0i32; 384];
    for (c, slot) in classes.iter_mut().enumerate() {
        let ci = c as c_int;
        let sp = unsafe { libc::isspace(ci) != 0 };
        let dg = unsafe { libc::isdigit(ci) != 0 };
        *slot = u16::from(sp) | (u16::from(dg) << 1);
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        unsafe extern "C" {
            fn __ctype_tolower_loc() -> *mut *const i32;
        }
        let tbl = *__ctype_tolower_loc();
        for c in -128i32..256i32 {
            lower[(c + 128) as usize] = *tbl.offset(c as isize);
        }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    for c in -128i32..256i32 {
        lower[(c + 128) as usize] = if c < 0 {
            c
        } else {
            unsafe { libc::tolower(c) }
        };
    }
    let ct = Ctype {
        classes: &classes,
        space: 1,
        digit: 2,
        lower: &lower,
        char_signed: (0 as c_char).wrapping_sub(1) < 0,
    };
    f(ct)
}

#[cfg(unix)]
pub(crate) fn parse_and_materialize_nzb(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    filename: &[u8],
    category: &[u8],
) -> ParsedMaterializedNzb {
    use crate::queuesubsys_host::{
        FILE_ACT_CLEAR_ARTICLES, MK_ERROR, MK_WARNING, NZB_ACT_BUILD_DEST_DIR,
        NZB_ACT_UPDATE_MIN_MAX_TIME, NZB_INT_MANY_DUPE_FILES, NZB_STR_CATEGORY, NZB_STR_FILENAME,
    };

    crate::log::log_active_bytes(3, b"Creating NZBFile");
    let (uptr_addr, nzb_ptr) = host.alloc_new_nzb_uptr();
    host.nzb_set_str(nzb_ptr, NZB_STR_FILENAME, filename);
    host.nzb_set_str(nzb_ptr, NZB_STR_CATEGORY, category);
    host.nzb_action(nzb_ptr, NZB_ACT_BUILD_DEST_DIR, 0);

    let opts = host.opts();

    let mut parser = NzbFileParser::new(filename);
    let ok = with_libc_ctype(|ct| {
        parser.parse_file(
            &opts.par_ignore_ext,
            &ct,
            || host.alloc_file_id(),
            |subj: &CStr, fname: &CStr| {
                let mut m1 = b"Extracting a filename from Subject ".to_vec();
                m1.extend_from_slice(subj.to_bytes());
                crate::log::log_active_bytes(4, &m1[..m1.len().min(1023)]);
                let mut m2 = b"Extracted Filename: ".to_vec();
                m2.extend_from_slice(fname.to_bytes());
                crate::log::log_active_bytes(4, &m2[..m2.len().min(1023)]);
            },
            None::<fn(&[*const c_char], &mut [usize])>,
        )
    });

    for msg in &parser.messages {
        let mk = if msg.kind == 2 { MK_ERROR } else { MK_WARNING };
        host.nzb_add_message(nzb_ptr, mk, msg.text.to_bytes());
    }

    if !ok {
        return ParsedMaterializedNzb {
            ok: false,
            uptr_addr,
            nzb_ptr,
            password: Vec::new(),
            category_from_file: Vec::new(),
            files: Vec::new(),
        };
    }

    host.nzb_set_int(
        nzb_ptr,
        NZB_INT_MANY_DUPE_FILES,
        i32::from(parser.many_dupe_files),
    );
    let counters = crate::queuecoord::NzbgetRsNzbMergeSplitCounters {
        file_count: parser.file_count,
        total_articles: parser.total_articles,
        failed_articles: parser.failed_articles,
        current_failed_articles: parser.cur_failed_articles,
        size: parser.size,
        remaining_size: parser.remaining_size,
        failed_size: parser.failed_size,
        current_failed_size: parser.cur_failed_size,
        par_size: parser.par_size,
        par_failed_size: parser.par_failed_size,
        par_current_failed_size: parser.par_cur_failed_size,
        remaining_par_count: parser.remaining_par_count,
        full_content_hash: parser.full_content_hash,
        filtered_content_hash: parser.filtered_content_hash,
        ..Default::default()
    };
    host.nzb_apply_merge_split_counters(nzb_ptr, &counters);

    let mut files = Vec::with_capacity(parser.files.len());
    for kf in &parser.files {
        let file_ptr = host.nzb_add_parsed_file(
            nzb_ptr,
            kf.id,
            kf.subject.as_ptr(),
            kf.filename.as_ptr(),
            kf.time,
            kf.size,
            kf.remaining_size,
            kf.missed_size,
            kf.total_articles,
            kf.missed_articles,
            kf.par_file,
            &kf.group_ptrs,
            &kf.article_parts,
            &kf.article_sizes,
            &kf.article_msg_id_ptrs,
        );
        files.push(ParsedMaterializedFile {
            file_ptr,
            total_articles: kf.total_articles,
            missed_articles: kf.missed_articles,
            cut_short: kf.cut_short,
        });
    }

    host.nzb_action(nzb_ptr, NZB_ACT_UPDATE_MIN_MAX_TIME, 0);
    if opts.server_mode {
        for f in &files {
            host.disk_save_file(f.file_ptr);
            host.file_action(f.file_ptr, FILE_ACT_CLEAR_ARTICLES, 0);
        }
    }

    let meta_name = parser.meta_name.to_bytes();
    if !meta_name.is_empty() {
        host.nzb_set_meta_name(nzb_ptr, meta_name);
    }

    ParsedMaterializedNzb {
        ok: true,
        uptr_addr,
        nzb_ptr,
        password: parser.password.to_bytes().to_vec(),
        category_from_file: parser.category.to_bytes().to_vec(),
        files,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum XmlInputEncoding {
    Raw,
    Utf16Le,
    Utf16Be,
}

const XML_STREAM_CHUNK_SIZE: usize = 8192;

struct XmlStreamReader<R> {
    inner: R,
    encoding: XmlInputEncoding,
    raw_buf: Vec<u8>,
    raw_pos: usize,
    raw_len: usize,
    out_buf: Vec<u8>,
    out_pos: usize,
    eof: bool,
    empty: bool,
    checked_xml_decl: bool,
}

impl<R: std::io::Read> XmlStreamReader<R> {
    fn new(mut inner: R) -> std::io::Result<Self> {
        let mut raw_buf = vec![0u8; XML_STREAM_CHUNK_SIZE];
        let mut raw_len = 0usize;
        let mut eof = false;
        while raw_len < 4 {
            match inner.read(&mut raw_buf[raw_len..]) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(n) => raw_len += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        if raw_len == 0 {
            return Ok(Self {
                inner,
                encoding: XmlInputEncoding::Raw,
                raw_buf,
                raw_pos: 0,
                raw_len: 0,
                out_buf: Vec::new(),
                out_pos: 0,
                eof: true,
                empty: true,
                checked_xml_decl: true,
            });
        }

        let (encoding, raw_pos) = if raw_len >= 2
            && raw_buf[0] == 0xFF
            && raw_buf[1] == 0xFE
            && !(raw_len >= 4 && raw_buf[2] == 0x00 && raw_buf[3] == 0x00)
        {
            (XmlInputEncoding::Utf16Le, 2)
        } else if raw_len >= 2 && raw_buf[0] == 0xFE && raw_buf[1] == 0xFF {
            (XmlInputEncoding::Utf16Be, 2)
        } else if raw_len >= 4
            && raw_buf[0] == b'<'
            && raw_buf[1] == 0x00
            && raw_buf[2] != 0x00
            && raw_buf[3] == 0x00
        {
            (XmlInputEncoding::Utf16Le, 0)
        } else if raw_len >= 4
            && raw_buf[0] == 0x00
            && raw_buf[1] == b'<'
            && raw_buf[2] == 0x00
            && raw_buf[3] != 0x00
        {
            (XmlInputEncoding::Utf16Be, 0)
        } else {
            (XmlInputEncoding::Raw, 0)
        };

        let mut out_buf = Vec::new();
        let mut final_raw_pos = raw_pos;
        let mut final_raw_len = raw_len;
        if encoding == XmlInputEncoding::Raw {
            out_buf.extend_from_slice(&raw_buf[..raw_len]);
            final_raw_pos = 0;
            final_raw_len = 0;
        }

        Ok(Self {
            inner,
            encoding,
            raw_buf,
            raw_pos: final_raw_pos,
            raw_len: final_raw_len,
            out_buf,
            out_pos: 0,
            eof,
            empty: false,
            checked_xml_decl: encoding == XmlInputEncoding::Raw,
        })
    }

    fn is_empty(&self) -> bool {
        self.empty
    }

    fn rewrite_utf16_xml_decl(buf: &mut Vec<u8>) {
        let start = if buf.starts_with(b"\xEF\xBB\xBF") {
            3
        } else {
            0
        };
        let rest = &buf[start..];
        let trimmed_offset = rest
            .iter()
            .position(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            .unwrap_or(rest.len());
        let decl_start = start + trimmed_offset;
        if !buf[decl_start..].starts_with(b"<?xml") {
            return;
        }
        let Some(end_rel) = buf[decl_start..].windows(2).position(|w| w == b"?>") else {
            return;
        };
        let decl_end = decl_start + end_rel + 2;
        let decl = &buf[decl_start..decl_end];
        let Some(enc_rel) = decl.windows(8).position(|w| w == b"encoding") else {
            return;
        };
        let mut i = decl_start + enc_rel + 8;
        while i < decl_end && matches!(buf[i], b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
        }
        if i >= decl_end || buf[i] != b'=' {
            return;
        }
        i += 1;
        while i < decl_end && matches!(buf[i], b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
        }
        if i >= decl_end || !matches!(buf[i], b'"' | b'\'') {
            return;
        }
        let quote = buf[i];
        let val_start = i + 1;
        let Some(val_len) = buf[val_start..decl_end].iter().position(|&b| b == quote) else {
            return;
        };
        let val_end = val_start + val_len;
        let val = &buf[val_start..val_end];
        let is_utf16_family = val.len() >= 6 && val[..6].eq_ignore_ascii_case(b"utf-16")
            || val.eq_ignore_ascii_case(b"ucs-2")
            || val.eq_ignore_ascii_case(b"unicode");
        if is_utf16_family {
            buf.splice(val_start..val_end, b"UTF-8".iter().copied());
        }
    }

    fn refill_out_buf(&mut self) -> std::io::Result<()> {
        self.out_buf.clear();
        self.out_pos = 0;

        if self.encoding == XmlInputEncoding::Raw {
            if self.eof {
                return Ok(());
            }
            loop {
                match self.inner.read(&mut self.raw_buf) {
                    Ok(0) => {
                        self.eof = true;
                        return Ok(());
                    }
                    Ok(n) => {
                        self.out_buf.extend_from_slice(&self.raw_buf[..n]);
                        return Ok(());
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
        }

        let min_bytes = if !self.checked_xml_decl { 512 } else { 4 };
        while self.raw_len - self.raw_pos < min_bytes && !self.eof {
            if self.raw_pos > 0 {
                let rem = self.raw_len - self.raw_pos;
                self.raw_buf.copy_within(self.raw_pos..self.raw_len, 0);
                self.raw_len = rem;
                self.raw_pos = 0;
            }
            match self.inner.read(&mut self.raw_buf[self.raw_len..]) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(n) => self.raw_len += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }

        if self.raw_pos == self.raw_len {
            return Ok(());
        }
        if self.eof && (self.raw_len - self.raw_pos) % 2 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Odd byte count in UTF-16 XML input",
            ));
        }

        let mut char_buf = [0u8; 4];
        while self.raw_len - self.raw_pos >= 2 {
            let b0 = self.raw_buf[self.raw_pos];
            let b1 = self.raw_buf[self.raw_pos + 1];
            let u = match self.encoding {
                XmlInputEncoding::Utf16Le => u16::from_le_bytes([b0, b1]),
                XmlInputEncoding::Utf16Be => u16::from_be_bytes([b0, b1]),
                XmlInputEncoding::Raw => unreachable!(),
            };
            if (0xD800..=0xDBFF).contains(&u) {
                if self.raw_len - self.raw_pos < 4 {
                    if self.eof {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Unpaired high surrogate at EOF in UTF-16 XML input",
                        ));
                    }
                    break;
                }
                let b2 = self.raw_buf[self.raw_pos + 2];
                let b3 = self.raw_buf[self.raw_pos + 3];
                let u2 = match self.encoding {
                    XmlInputEncoding::Utf16Le => u16::from_le_bytes([b2, b3]),
                    XmlInputEncoding::Utf16Be => u16::from_be_bytes([b2, b3]),
                    XmlInputEncoding::Raw => unreachable!(),
                };
                if !(0xDC00..=0xDFFF).contains(&u2) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Unpaired high surrogate in UTF-16 XML input",
                    ));
                }
                let cp = 0x10000 + (((u as u32 - 0xD800) << 10) | (u2 as u32 - 0xDC00));
                let ch = char::from_u32(cp).unwrap();
                let encoded = ch.encode_utf8(&mut char_buf);
                self.out_buf.extend_from_slice(encoded.as_bytes());
                self.raw_pos += 4;
            } else if (0xDC00..=0xDFFF).contains(&u) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Unpaired low surrogate in UTF-16 XML input",
                ));
            } else {
                let ch = char::from_u32(u as u32).unwrap();
                let encoded = ch.encode_utf8(&mut char_buf);
                self.out_buf.extend_from_slice(encoded.as_bytes());
                self.raw_pos += 2;
            }
        }

        if !self.checked_xml_decl {
            self.checked_xml_decl = true;
            Self::rewrite_utf16_xml_decl(&mut self.out_buf);
        }

        Ok(())
    }
}

impl<R: std::io::Read> std::io::Read for XmlStreamReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let available = std::io::BufRead::fill_buf(self)?;
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        std::io::BufRead::consume(self, n);
        Ok(n)
    }
}

impl<R: std::io::Read> std::io::BufRead for XmlStreamReader<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.out_pos >= self.out_buf.len() {
            self.refill_out_buf()?;
        }
        Ok(&self.out_buf[self.out_pos..])
    }

    fn consume(&mut self, amt: usize) {
        self.out_pos = (self.out_pos + amt).min(self.out_buf.len());
    }
}

/// Computes the trimmed length of an XML/SAX error message after stripping trailing
/// `\n`, `\r`, and `' '` (`NzbFile::SAX_error`).
pub fn nzbfile_trim_sax_error_trailing(buf: &[u8]) -> usize {
    let mut len = buf.len();
    while len > 0 && matches!(buf[len - 1], b'\n' | b'\r' | b' ') {
        len -= 1;
    }
    len
}

/// Formats `"Error parsing nzb-file <base_name>"` or
/// `"Error parsing nzb-file <base_name>: file has no content"` for `NzbFile::Parse`.
pub fn nzbfile_format_parse_error_msg(base_name: &[u8], has_no_content: bool) -> Vec<u8> {
    const PREFIX: &[u8] = b"Error parsing nzb-file ";
    const NO_CONTENT_SUFFIX: &[u8] = b": file has no content";
    let extra = if has_no_content {
        NO_CONTENT_SUFFIX.len()
    } else {
        0
    };
    let mut out = Vec::with_capacity(PREFIX.len() + base_name.len() + extra);
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(base_name);
    if has_no_content {
        out.extend_from_slice(NO_CONTENT_SUFFIX);
    }
    out
}

/// Formats `"Error parsing nzb-file: <err_msg>"` for `NzbFile::SAX_error`.
pub fn nzbfile_format_sax_error_msg(err_msg: &[u8]) -> Vec<u8> {
    const PREFIX: &[u8] = b"Error parsing nzb-file: ";
    let mut out = Vec::with_capacity(PREFIX.len() + err_msg.len());
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(err_msg);
    out
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
            assert_eq!(declared_tail(b"a.rar yEnc (1/3709)", 1, &full[..1], ct), 0);
            let mixed: Vec<i64> = (0..20).map(|i| 700_000 + i * 1000).collect();
            assert_eq!(declared_tail(b"a.rar yEnc (1/3709)", 20, &mixed, ct), 3709);
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
            assert_eq!(
                parser.files[0].article_msg_ids[0].as_bytes(),
                b"<msg1@test>"
            );
            assert_eq!(
                parser.files[0].article_msg_ids[1].as_bytes(),
                b"<msg2__@test>"
            );
        });
    }

    #[test]
    fn nzbfile_full_suite_port_from_cpp() {
        with_c_locale(|ct| {
            // 1. NZBParserTest over tests/testdata/nzbfile/*.nzb
            let testdata_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("tests/testdata/nzbfile");
            let cases = [
                ("dotless", "Movies"),
                ("plain", "TV>4K"),
                ("literal_gt", "TV > 4K"),
                ("passwd{{thisisthepassword}}", ""),
                ("passwdMeta", ""),
                ("passwdEntity", ""),
            ];
            for (test_filename, expected_category) in cases {
                let nzb_path = testdata_dir.join(format!("{test_filename}.nzb"));
                let info_path = testdata_dir.join(format!("{test_filename}.txt"));
                let mut next_id = 1i32;
                let mut parser = NzbFileParser::new(nzb_path.to_str().unwrap().as_bytes());
                assert!(
                    parser.parse_file(
                        b".nfo",
                        ct,
                        || {
                            let id = next_id;
                            next_id += 1;
                            id
                        },
                        |_, _| {},
                        None::<fn(&[*const c_char], &mut [usize])>
                    ),
                    "failed to parse {test_filename}.nzb"
                );
                assert_eq!(
                    parser.category.to_str().unwrap(),
                    expected_category,
                    "category mismatch for {test_filename}"
                );
                let info_text = std::fs::read_to_string(&info_path).unwrap();
                let non_comment_lines: Vec<&str> = info_text
                    .lines()
                    .filter(|l| !l.starts_with('#'))
                    .map(|l| l.trim_end_matches(['\r', '\n', ' ', '\t']))
                    .collect();
                let file_count: usize = non_comment_lines[0].parse().unwrap();
                assert_eq!(parser.file_count as usize, file_count);
                assert_eq!(parser.files.len(), file_count);
                for i in 0..file_count {
                    assert_eq!(
                        parser.files[i].filename.to_str().unwrap(),
                        non_comment_lines[1 + i]
                    );
                }
                let expected_pw = non_comment_lines.get(1 + file_count).copied().unwrap_or("");
                assert_eq!(parser.password.to_str().unwrap(), expected_pw);
            }

            // 2. SanitizePathSegmentTest
            use crate::paths::{sanitize_path_segment, sanitize_relative_path, PATH_SEPARATOR};
            assert_eq!(
                sanitize_path_segment(b"Clean.Release.Name"),
                b"Clean.Release.Name"
            );
            let test_path = sanitize_path_segment(b"../../etc/cron.d");
            assert!(!test_path.contains(&b'/'));
            assert!(!test_path.contains(&b'\\'));
            assert!(!test_path.windows(2).any(|w| w == b".."));
            assert_eq!(sanitize_path_segment(b".."), b"");
            assert_eq!(sanitize_path_segment(b"."), b"");
            assert_eq!(sanitize_path_segment(b""), b"");
            assert_eq!(
                sanitize_path_segment(b"name:with*bad?chars"),
                b"name_with_bad_chars"
            );
            assert_eq!(
                sanitize_path_segment(b"folder/sub\\name"),
                b"folder_sub_name"
            );
            assert_eq!(
                sanitize_path_segment(b"trailing.dots...   "),
                b"trailing.dots"
            );
            assert_eq!(sanitize_path_segment(b"   leading.space"), b"leading.space");
            assert_eq!(sanitize_path_segment(b"CON"), b"_CON");
            assert_eq!(sanitize_path_segment(b"NUL.txt"), b"_NUL.txt");
            assert_eq!(sanitize_path_segment(b"AUX"), b"_AUX");
            assert_eq!(sanitize_path_segment(b"COM1"), b"_COM1");
            assert_eq!(sanitize_path_segment(b"PRN.dat"), b"_PRN.dat");
            assert_eq!(sanitize_path_segment(b"LPT1"), b"_LPT1");
            assert_eq!(
                sanitize_path_segment("Vidéo.2026.mkv".as_bytes()),
                "Vidéo.2026.mkv".as_bytes()
            );
            assert_eq!(
                sanitize_path_segment("Mädchen.mp4".as_bytes()),
                "Mädchen.mp4".as_bytes()
            );
            assert_eq!(sanitize_path_segment(b"..."), b"");
            assert_eq!(sanitize_path_segment(b".. "), b"");
            assert_eq!(
                sanitize_path_segment(b"prefix..middle...suffix"),
                b"prefix_middle_suffix"
            );
            let dot_path1 = sanitize_path_segment(b".../");
            assert!(!dot_path1.contains(&b'/'));
            assert!(!dot_path1.windows(2).any(|w| w == b".."));
            let dot_path2 = sanitize_path_segment(b"folder/...");
            assert!(!dot_path2.contains(&b'/'));
            assert!(!dot_path2.windows(2).any(|w| w == b".."));
            assert_eq!(sanitize_path_segment(&vec![b'.'; 10_000]), b"");
            assert_eq!(sanitize_path_segment(&vec![b'a'; 2000]).len(), 1024);
            let mut split_utf8 = vec![b'a'; 1023];
            split_utf8.extend_from_slice(b"\xC3\xA9tail");
            let clean_split = sanitize_path_segment(&split_utf8);
            assert_eq!(clean_split.len(), 1023);
            assert_eq!(clean_split.last(), Some(&b'a'));
            let mut intact_utf8 = vec![b'a'; 1022];
            intact_utf8.extend_from_slice(b"\xC3\xA9tail");
            let clean_intact = sanitize_path_segment(&intact_utf8);
            assert_eq!(clean_intact.len(), 1024);
            assert_eq!(&clean_intact[1022..], b"\xC3\xA9");
            assert_eq!(sanitize_path_segment(b"C:\\Windows"), b"C__Windows");
            assert_eq!(
                sanitize_path_segment(b"\\\\server\\share"),
                b"__server_share"
            );

            // 3. SanitizeRelativePathTest
            let sep = PATH_SEPARATOR as char;
            let srel = |s: &str| String::from_utf8(sanitize_relative_path(s.as_bytes())).unwrap();
            assert_eq!(srel("Movies"), "Movies");
            assert_eq!(srel("TV/HD"), format!("TV{sep}HD"));
            assert_eq!(srel("TV / HD"), format!("TV{sep}HD"));
            assert_eq!(srel("TV>HD"), "TV_HD");
            assert_eq!(srel("TV > HD"), "TV _ HD");
            assert_eq!(srel("TV\\Shows"), format!("TV{sep}Shows"));
            assert_eq!(srel("TV/../../HD"), format!("TV{sep}HD"));
            assert_eq!(srel("TV/./HD"), format!("TV{sep}HD"));
            assert_eq!(srel("TV/.."), "TV");
            assert_eq!(srel("TV/../../../../../etc"), format!("TV{sep}etc"));
            assert_eq!(srel("../../Movies/4K"), format!("Movies{sep}4K"));
            assert_eq!(srel("/var/www/"), format!("var{sep}www"));
            assert_eq!(
                srel("C:\\Movies\\SciFi"),
                format!("C_{sep}Movies{sep}SciFi")
            );
            assert_eq!(srel("Séries/HD"), format!("Séries{sep}HD"));
            assert_eq!(srel(".."), "");
            assert_eq!(srel("../.."), "");
            assert_eq!(srel(""), "");
            assert_eq!(srel("myfolder/.../file"), format!("myfolder{sep}file"));
            assert_eq!(srel("myfolder/.. /file"), format!("myfolder{sep}file"));
            assert_eq!(srel(".../file"), "file");
            assert_eq!(
                srel("\\\\remote-server\\share\\data"),
                format!("remote-server{sep}share{sep}data")
            );
            assert_eq!(
                srel("//remote-server/share/data"),
                format!("remote-server{sep}share{sep}data")
            );
            assert_eq!(
                srel("C:\\Windows\\System32"),
                format!("C_{sep}Windows{sep}System32")
            );
            assert_eq!(
                srel("C:/Windows/System32"),
                format!("C_{sep}Windows{sep}System32")
            );
            assert_eq!(srel("LPT1/subfolder"), format!("_LPT1{sep}subfolder"));
            assert_eq!(srel("movies/CON/item"), format!("movies{sep}_CON{sep}item"));

            // Helper to parse inline NZB with given <head> meta content
            let parse_head_nzb = |head_meta: &str| -> NzbFileParser {
                let xml = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <!DOCTYPE nzb PUBLIC \"-//newzBin//DTD NZB 1.0//EN\" \"http://www.newzbin.com/DTD/nzb/nzb-1.0.dtd\">\n\
                     <nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n\
                     <head>\n{head_meta}\n</head>\n\
                     <file poster=\"poster@test.com\" date=\"1335508618\" subject=\"&quot;testfile.mkv&quot; yEnc (1/1)\">\n\
                     <groups><group>alt.binaries.test</group></groups>\n\
                     <segments><segment bytes=\"100\" number=\"1\">test@msgid</segment></segments>\n\
                     </file>\n</nzb>\n"
                );
                let mut p = NzbFileParser::new(b"/tmp/test.nzb");
                let ret = p.parse_xml(xml.as_bytes(), || 1);
                assert!(p.finalize(ret, b".nfo", ct));
                p
            };

            // 4. NzbFileMetaParsingTest
            let p = parse_head_nzb(
                "<meta type=\"name\">My.Clean.Release.Name.S01E01 &amp; Special.Edition</meta>\n\
                 <meta type=\"title\">My.Clean.Release.Name.S01E01.mkv</meta>\n\
                 <meta type=\"category\">TV / HD</meta>\n\
                 <meta type=\"password\">secret123</meta>",
            );
            assert_eq!(
                p.meta_name.to_str().unwrap(),
                "My.Clean.Release.Name.S01E01 & Special.Edition"
            );
            assert_eq!(
                p.meta_title.to_str().unwrap(),
                "My.Clean.Release.Name.S01E01.mkv"
            );
            assert_eq!(p.password.to_str().unwrap(), "secret123");
            assert_eq!(p.category.to_str().unwrap(), "TV / HD");

            // 5. NzbFileMetaTitleFallbackTest
            let p = parse_head_nzb("<meta type=\"title\">Fallback.Release.Title.mkv</meta>");
            assert_eq!(p.meta_name.to_str().unwrap(), "Fallback.Release.Title.mkv");
            assert_eq!(p.meta_title.to_str().unwrap(), "Fallback.Release.Title.mkv");

            // 6. NzbFileMetaPathValidationTest
            let p = parse_head_nzb(
                "<meta type=\"name\">../../../../test/folder/release</meta>\n\
                 <meta type=\"category\">../../../../media/downloads</meta>",
            );
            let meta_name_str = p.meta_name.to_str().unwrap();
            assert!(!meta_name_str.contains('/'));
            assert!(!meta_name_str.contains('\\'));
            assert!(!meta_name_str.contains(".."));
            assert_eq!(
                srel(p.category.to_str().unwrap()),
                format!("media{sep}downloads")
            );

            // 7. NzbFileCategoryControlCharsTest
            let p = parse_head_nzb("<meta type=\"category\">\r\n\t  Movies\r\n/\t4K  \r\n</meta>");
            let cat_str = p.category.to_str().unwrap();
            assert!(!cat_str.contains('\n'));
            assert!(!cat_str.contains('\r'));
            assert!(!cat_str.contains('\t'));
            assert_eq!(srel(cat_str), format!("Movies{sep}4K"));

            // 8. NzbFileSegmentNumbersTest
            let seg_xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                <nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n\
                <file poster=\"p\" date=\"1335508618\" subject=\"&quot;a.mkv&quot; yEnc (1/4)\">\n\
                <groups><group>alt.binaries.test</group></groups><segments>\n\
                <segment bytes=\"100\" number=\"4\">d@x</segment>\n\
                <segment bytes=\"100\" number=\"1\">a@x</segment>\n\
                <segment bytes=\"100\" number=\"1\">a2@x</segment>\n\
                <segment bytes=\"100\" number=\"2\">b@x</segment>\n\
                </segments></file>\n\
                <file poster=\"p\" date=\"1335508618\" subject=\"&quot;b.mkv&quot; yEnc (1/1)\">\n\
                <groups><group>alt.binaries.test</group></groups><segments>\n\
                <segment bytes=\"100\" number=\"2000000000\">z@x</segment>\n\
                </segments></file>\n\
                </nzb>\n";
            let mut p = NzbFileParser::new(b"/tmp/seg.nzb");
            let mut next_id = 1i32;
            let ret = p.parse_xml(seg_xml, || {
                let id = next_id;
                next_id += 1;
                id
            });
            assert!(p.finalize(ret, b".nfo", ct));
            assert_eq!(p.files.len(), 2);
            let first = p.files.iter().find(|f| f.total_articles == 4).unwrap();
            let second = p.files.iter().find(|f| f.total_articles != 4).unwrap();
            assert_eq!(first.article_msg_ids.len(), 3);
            assert_eq!(first.article_msg_ids[0].as_bytes(), b"<a2@x>");
            assert_eq!(first.article_msg_ids[1].as_bytes(), b"<b@x>");
            assert_eq!(first.article_msg_ids[2].as_bytes(), b"<d@x>");
            assert_eq!(first.missed_articles, 1);
            assert_eq!(first.size, 400);
            assert_eq!(first.missed_size, 100);
            assert_eq!(second.article_msg_ids.len(), 1);
            assert_eq!(second.total_articles, 2_000_000_000);

            // 9. EmptyNzbNameFallbackTest
            use crate::downloadinfo::{build_final_dir_name, make_nice_nzb_name};
            assert_eq!(make_nice_nzb_name(b"   .nzb", true), b"nzb");
            assert_eq!(make_nice_nzb_name(b".nzb", true), b"nzb");
            assert_eq!(make_nice_nzb_name(b"...", true), b"nzb");
            assert_eq!(make_nice_nzb_name(b"   ", true), b"nzb");
            assert_eq!(make_nice_nzb_name(b"", true), b"nzb");
            assert_eq!(make_nice_nzb_name(b"   .nzb", false), b".nzb");
            assert_eq!(make_nice_nzb_name(b"My.Release.nzb", true), b"My.Release");
            assert_eq!(
                make_nice_nzb_name(b"My.Release.nzb", false),
                b"My.Release.nzb"
            );
            assert_eq!(make_nice_nzb_name(b"My.Release", false), b"My.Release");
            assert_eq!(
                make_nice_nzb_name(b"My..Release...nzb", true),
                b"My_Release"
            );

            // 10. BuildFinalDirNameUniqueIdTest
            let final_dir1 = String::from_utf8(build_final_dir_name(
                b"/downloads/dst",
                b"",
                b"",
                false,
                b"",
                101,
            ))
            .unwrap();
            let final_dir2 = String::from_utf8(build_final_dir_name(
                b"/downloads/dst",
                b"",
                b"",
                false,
                b"",
                102,
            ))
            .unwrap();
            assert!(final_dir1.ends_with("nzb-101"));
            assert!(final_dir2.ends_with("nzb-102"));
            assert_ne!(final_dir1, final_dir2);

            // 11. DeclaredPartsTest
            assert_eq!(declared_parts(b"\"movie.mkv\" yEnc (1/3709)", ct), 3709);
            assert_eq!(declared_parts(b"[01/12] - \"a.rar\" yEnc (1/50) ", ct), 50);
            assert_eq!(declared_parts(b"[01/12] - \"a.rar\" yEnc", ct), 0);
            assert_eq!(declared_parts(b"\"a.rar\" yEnc (1/50) 52428800", ct), 0);
            assert_eq!(declared_parts(b"(/50)", ct), 0);
            assert_eq!(declared_parts(b"(1/)", ct), 0);
            assert_eq!(declared_parts(b"1/50)", ct), 0);
            assert_eq!(declared_parts(b"x (1/99999999999)", ct), 0);
            assert_eq!(declared_parts(b"", ct), 0);

            // 12. PartlyListedFileTest
            let partly_xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                <nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n\
                <file poster=\"p\" date=\"1\" subject=\"&quot;movie.mkv&quot; yEnc (1/300)\">\n\
                <groups><group>alt.binaries.test</group></groups>\n<segments>\n\
                <segment bytes=\"100\" number=\"1\">a1@test</segment>\n\
                <segment bytes=\"100\" number=\"2\">a2@test</segment>\n\
                <segment bytes=\"100\" number=\"4\">a4@test</segment>\n\
                </segments>\n</file>\n</nzb>\n";
            let mut p = NzbFileParser::new(b"/tmp/partly.nzb");
            let ret = p.parse_xml(partly_xml, || 1);
            assert!(p.finalize(ret, b".nfo", ct));
            assert_eq!(p.files.len(), 1);
            assert_eq!(p.files[0].total_articles, 300);
            assert_eq!(p.files[0].missed_articles, 297);
            assert_eq!(p.files[0].size, 30_000);
            assert_eq!(p.files[0].missed_size, 29_700);
            assert_eq!(p.failed_size, 29_700);

            // 13. DeclaredTailGuardTest
            let parse_tail = |name: &str, declared: i32, segments: &[(i32, i32)]| -> i32 {
                let mut xml = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n\
                     <file poster=\"p\" date=\"1\" subject=\"&quot;{name}&quot; yEnc (1/{declared})\">\n\
                     <groups><group>alt.binaries.test</group></groups>\n<segments>\n"
                );
                for &(number, bytes) in segments {
                    xml.push_str(&format!(
                        "<segment bytes=\"{bytes}\" number=\"{number}\">a{number}@test</segment>\n"
                    ));
                }
                xml.push_str("</segments>\n</file>\n</nzb>\n");
                let mut p = NzbFileParser::new(b"/tmp/tail.nzb");
                let ret = p.parse_xml(xml.as_bytes(), || 1);
                assert!(p.finalize(ret, b".nfo", ct));
                p.files[0].total_articles
            };
            let mut nine: Vec<(i32, i32)> = (1..=9).map(|i| (i, 100)).collect();
            assert_eq!(parse_tail("a.rar", 10, &nine), 9);
            assert_eq!(parse_tail("a.rar", 100, &nine), 9);
            assert_eq!(parse_tail("a.rar", 200, &nine), 200);
            assert_eq!(parse_tail("a.vol00+10.par2", 200, &nine), 9);
            nine.last_mut().unwrap().1 = 99;
            assert_eq!(parse_tail("a.rar", 200, &nine), 9);
            let mut varied = vec![(1, 1000), (2, 1003), (3, 998), (4, 995)];
            assert_eq!(parse_tail("a.mkv", 300, &varied), 300);
            varied.last_mut().unwrap().1 = 960;
            assert_eq!(parse_tail("a.mkv", 300, &varied), 4);
            assert_eq!(parse_tail("a.rar", 200, &[(1, 100)]), 1);

            // 14. MessageIdControlCharsTest
            let msgid_xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                <nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n\
                <file poster=\"p\" date=\"1\" subject=\"&quot;a.rar&quot; yEnc (1/1)\">\n\
                <groups><group>alt.binaries.test</group></groups>\n<segments>\n\
                <segment bytes=\"100\" number=\"1\">a&#13;&#10;QUIT&#13;&#10;b@test</segment>\n\
                </segments>\n</file>\n</nzb>\n";
            let mut p = NzbFileParser::new(b"/tmp/msgid.nzb");
            let ret = p.parse_xml(msgid_xml, || 1);
            assert!(p.finalize(ret, b".nfo", ct));
            assert_eq!(p.files[0].article_msg_ids.len(), 1);
            let id = p.files[0].article_msg_ids[0].to_str().unwrap();
            assert!(!id.contains('\r') && !id.contains('\n'));
            assert_eq!(id, "<a__QUIT__b@test>");

            // 15. Utf16AndStreamingReaderTest
            let encode_utf16 = |s: &str, le: bool, bom: bool| -> Vec<u8> {
                let mut out = Vec::new();
                if bom {
                    out.extend_from_slice(if le { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
                }
                for u in s.encode_utf16() {
                    let bytes = if le { u.to_le_bytes() } else { u.to_be_bytes() };
                    out.extend_from_slice(&bytes);
                }
                out
            };

            let mut utf16_doc = String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n\
                 <nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n\
                 <head><meta type=\"password\">s\u{00E9}cret-\u{1F512}</meta></head>\n\
                 <file poster=\"p\u{00FC}ster\" date=\"123\" subject=\"&quot;f\u{00FC}le-\u{1F680}.rar&quot; yEnc (1/120)\">\n\
                 <groups><group>alt.binaries.utf16</group></groups>\n<segments>\n",
            );
            for i in 1..=120 {
                utf16_doc.push_str(&format!(
                    "<segment bytes=\"1000\" number=\"{i}\">seg-{i}-\u{1F680}@utf16.example</segment>\n"
                ));
            }
            utf16_doc.push_str("</segments>\n</file>\n</nzb>\n");
            assert!(utf16_doc.len() > XML_STREAM_CHUNK_SIZE);

            for (le, bom, decl_enc) in [
                (true, true, "UTF-16"),
                (false, true, "UTF-16"),
                (true, false, "UTF-16LE"),
                (false, false, "UTF-16BE"),
            ] {
                let doc =
                    utf16_doc.replace("encoding=\"UTF-16\"", &format!("encoding=\"{decl_enc}\""));
                let bytes = encode_utf16(&doc, le, bom);
                let tmp_path = std::env::temp_dir().join(format!(
                    "nzbget_utf16_{}_{}_{}.nzb",
                    std::process::id(),
                    if le { "le" } else { "be" },
                    if bom { "bom" } else { "nobom" }
                ));
                std::fs::write(&tmp_path, &bytes).unwrap();
                let mut next_id = 0;
                let mut p = NzbFileParser::new(tmp_path.to_string_lossy().as_bytes());
                assert!(p.parse_file(
                    b".nfo",
                    ct,
                    || {
                        next_id += 1;
                        next_id
                    },
                    |_, _| {},
                    None::<fn(&[*const c_char], &mut [usize])>,
                ));
                let _ = std::fs::remove_file(&tmp_path);
                assert_eq!(p.password.to_str().unwrap(), "s\u{00E9}cret-\u{1F512}");
                assert_eq!(p.files.len(), 1);
                assert_eq!(
                    p.files[0].subject.to_str().unwrap(),
                    "\"f\u{00FC}le-\u{1F680}.rar\" yEnc (1/120)"
                );
                assert_eq!(
                    p.files[0].filename.to_str().unwrap(),
                    "f\u{00FC}le-\u{1F680}.rar"
                );
                assert_eq!(p.files[0].total_articles, 120);
                assert_eq!(p.files[0].size, 120_000);
            }

            // Invalid UTF-16 (odd byte count and unpaired surrogate) must fail cleanly
            let mut truncated_utf16 = encode_utf16(&utf16_doc, true, true);
            truncated_utf16.push(0x41);
            let mut p_trunc = NzbFileParser::new(b"/tmp/trunc_utf16.nzb");
            let ret_trunc = p_trunc.parse_xml(&truncated_utf16, || 1);
            assert!(!p_trunc.finalize(ret_trunc, b".nfo", ct));
            assert!(!p_trunc.messages.is_empty());

            let mut unpaired_surrogate = vec![0xFF, 0xFE, b'<', 0x00, b'a', 0x00, b'>', 0x00];
            unpaired_surrogate.extend_from_slice(&0xD800u16.to_le_bytes());
            unpaired_surrogate.extend_from_slice(&[b'<', 0x00, b'/', 0x00, b'a', 0x00, b'>', 0x00]);
            let mut p_surr = NzbFileParser::new(b"/tmp/surr_utf16.nzb");
            let ret_surr = p_surr.parse_xml(&unpaired_surrogate, || 1);
            assert!(!p_surr.finalize(ret_surr, b".nfo", ct));
            assert!(!p_surr.messages.is_empty());
        });
    }
}
