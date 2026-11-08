//! End-to-end Rust implementation of `daemon/feed/` (`FeedInfo`, `FeedItemInfo`,
//! `FeedFile`, `NzbInfoCreator`, and `FeedCoordinator`).

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

fn xml_digit_lower(byte: u8) -> i32 {
    i32::from(byte.to_ascii_lowercase())
}

fn text_right_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn xml_entity_alpha(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

fn trim_string_bytes(bytes: &[u8]) -> Vec<u8> {
    let (start, end) = crate::util::trim_string(bytes, true, true, text_right_space);
    bytes[start..end].to_vec()
}

/// Parses a signed 32-bit integer matching C `atoi` truncation semantics.
pub fn c_atoi_bytes(bytes: &[u8]) -> i32 {
    c_atoll_bytes(bytes) as i32
}

/// Parses a signed 32-bit integer clamping out-of-range values to `i32::MIN..=i32::MAX`.
pub fn c_atoi_clamped_bytes(bytes: &[u8]) -> i32 {
    c_atoll_bytes(bytes).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Parses a signed 64-bit integer matching C `atoll` semantics with signed-limit saturation.
pub fn c_atoll_bytes(bytes: &[u8]) -> i64 {
    let mut idx = 0;
    while idx < bytes.len() && matches!(bytes[idx], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        idx += 1;
    }
    let mut neg = false;
    if idx < bytes.len() {
        if bytes[idx] == b'-' {
            neg = true;
            idx += 1;
        } else if bytes[idx] == b'+' {
            idx += 1;
        }
    }
    let mut acc: u64 = 0;
    let mut any = false;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        any = true;
        acc = acc
            .saturating_mul(10)
            .saturating_add(u64::from(bytes[idx] - b'0'));
        idx += 1;
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

// ============================================================================
// 1. FeedInfo & FeedItemInfo (`daemon/feed/FeedInfo.cpp`)
// ============================================================================

/// Initializes a `FeedInfo`'s default name (`"Feed<id>"` when `name` is empty)
/// and calculates its BJ96 filter hash.
pub fn feed_info_init(id: i32, name: &[u8], filter: &[u8]) -> (Option<Vec<u8>>, u32) {
    let default_name = if name.is_empty() {
        Some(format!("Feed{id}").into_bytes())
    } else {
        None
    };
    let filter_hash = crate::util::hash_bj96(filter, 0);
    (default_name, filter_hash)
}

/// Compares two C attribute names case-insensitively (`FeedItemInfo::Attributes::Find`).
///
/// # Safety
/// Both pointers must be null or valid null-terminated C strings.
pub unsafe fn attr_name_eq_ignore_case(
    attr_name: *const c_char,
    target_name: *const c_char,
) -> bool {
    if attr_name.is_null() || target_name.is_null() {
        return false;
    }
    CStr::from_ptr(attr_name)
        .to_bytes()
        .eq_ignore_ascii_case(CStr::from_ptr(target_name).to_bytes())
}

/// Parses an integer optionally prefixed by a single non-digit character (`FeedItemInfo::ParsePrefixedInt`),
/// e.g. `"S03"` -> `3`, `"E69"` -> `69`, `"12"` -> `12`.
pub fn parse_prefixed_int(value: &[u8]) -> i32 {
    if value.is_empty() {
        return 0;
    }
    let slice = if !value[0].is_ascii_digit() {
        &value[1..]
    } else {
        value
    };
    c_atoi_clamped_bytes(slice)
}

/// Formats `Season` (`S%02d`) and `Episode` (`E` + `ep_raw` with `'-'` removed and
/// ASCII letters uppercased) for `FeedItemInfo::ParseSeasonEpisode`.
pub fn format_season_episode(season_num: i32, ep_raw: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let season = format!("S{season_num:02}").into_bytes();
    let mut episode = Vec::with_capacity(1 + ep_raw.len());
    episode.push(b'E');
    for &b in ep_raw {
        if b != b'-' {
            episode.push(b.to_ascii_uppercase());
        }
    }
    (season, episode)
}

/// Parses season and episode from `title` matching `FeedItemInfo::ParseSeasonEpisode`
/// (`"[^[:alnum:]]s?([0-9]+)[ex]([0-9]+(-?e[0-9]+)?)[^[:alnum:]]"`).
pub fn parse_season_episode(title: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut rx = crate::util::PosixRegEx::compile(
        c"[^[:alnum:]]s?([0-9]+)[ex]([0-9]+(-?e[0-9]+)?)[^[:alnum:]]",
        10,
    );
    let cstr = std::ffi::CString::new(title).ok()?;
    if !rx.match_cstr(&cstr) {
        return None;
    }
    let s1_start = rx.match_start(1).max(0) as usize;
    let s2_start = rx.match_start(2).max(0) as usize;
    let s2_len = rx.match_len(2).max(0) as usize;
    let season_slice = title.get(s1_start..)?;
    let season_num = crate::util::util_str_to_i64_checked(season_slice, i32::MIN as i64, i32::MAX as i64)
        .unwrap_or(0) as i32;
    let ep_slice = title.get(s2_start..s2_start.saturating_add(s2_len))?;
    Some(format_season_episode(season_num, ep_slice))
}

/// Evaluates whether a feed item has any series identifier (`series`, `rageid`, `tvdbid`, or `tvmazeid`).
pub fn has_any_series_id(
    series: &[u8],
    rage_id_str: &[u8],
    tvdb_id_str: &[u8],
    tvmaze_id_str: &[u8],
    item_rage_id: i32,
    item_tvdb_id: i32,
    item_tvmaze_id: i32,
) -> bool {
    let rage = if !rage_id_str.is_empty() {
        c_atoi_bytes(rage_id_str)
    } else {
        item_rage_id
    };
    let tvdb = if !tvdb_id_str.is_empty() {
        c_atoi_bytes(tvdb_id_str)
    } else {
        item_tvdb_id
    };
    let tvmaze = if !tvmaze_id_str.is_empty() {
        c_atoi_bytes(tvmaze_id_str)
    } else {
        item_tvmaze_id
    };
    !series.is_empty() || rage != 0 || tvdb != 0 || tvmaze != 0
}

/// Builds the duplicate key for `FeedItemInfo::BuildDupeKey`.
#[allow(clippy::too_many_arguments)]
pub fn build_feed_dupe_key(
    rage_id_str: &[u8],
    tvdb_id_str: &[u8],
    tvmaze_id_str: &[u8],
    series: &[u8],
    item_rage_id: i32,
    item_tvdb_id: i32,
    item_tvmaze_id: i32,
    item_imdb_id: i32,
    season_num: i32,
    episode_num: i32,
    season: &[u8],
    episode: &[u8],
) -> Vec<u8> {
    let rage_id_val = if !rage_id_str.is_empty() {
        c_atoi_clamped_bytes(rage_id_str)
    } else {
        item_rage_id
    };
    let tvdb_id_val = if !tvdb_id_str.is_empty() {
        c_atoi_clamped_bytes(tvdb_id_str)
    } else {
        item_tvdb_id
    };
    let tvmaze_id_val = if !tvmaze_id_str.is_empty() {
        c_atoi_clamped_bytes(tvmaze_id_str)
    } else {
        item_tvmaze_id
    };

    let has_se = season_num != 0 && episode_num != 0;
    if !series.is_empty() && has_se {
        let mut out = Vec::with_capacity(7 + series.len() + 1 + season.len() + 1 + episode.len());
        out.extend_from_slice(b"series=");
        out.extend_from_slice(series);
        out.push(b'-');
        out.extend_from_slice(season);
        out.push(b'-');
        out.extend_from_slice(episode);
        out
    } else if rage_id_val != 0 && has_se {
        let mut out = format!("rageid={rage_id_val}-").into_bytes();
        out.extend_from_slice(season);
        out.push(b'-');
        out.extend_from_slice(episode);
        out
    } else if tvdb_id_val != 0 && has_se {
        let mut out = format!("tvdbid={tvdb_id_val}-").into_bytes();
        out.extend_from_slice(season);
        out.push(b'-');
        out.extend_from_slice(episode);
        out
    } else if tvmaze_id_val != 0 && has_se {
        let mut out = format!("tvmazeid={tvmaze_id_val}-").into_bytes();
        out.extend_from_slice(season);
        out.push(b'-');
        out.extend_from_slice(episode);
        out
    } else if item_imdb_id != 0 {
        format!("imdb={item_imdb_id}").into_bytes()
    } else {
        Vec::new()
    }
}

// ============================================================================
// 2. FeedFileParser (`daemon/feed/FeedFile.cpp`)
// ============================================================================

/// Extracts a filename from an RSS item title (`FeedFile::ParseSubject`).
pub fn parse_feed_subject(title: &[u8]) -> Vec<u8> {
    if let Some(first_quote) = title.iter().position(|&b| b == b'"') {
        let start = first_quote + 1;
        if start + 1 <= title.len() {
            if let Some(rel_end) = title[start + 1..].iter().position(|&b| b == b'"') {
                let end = start + 1 + rel_end;
                let quoted = &title[start..end];
                if quoted.len() > 1 && quoted[1..].contains(&b'.') {
                    let mut filename = quoted;
                    if let Some(dot) = filename.iter().rposition(|&b| b == b'.') {
                        if filename[dot..].eq_ignore_ascii_case(b".par2") {
                            filename = &filename[..dot];
                        }
                    }
                    return filename.to_vec();
                }
            }
        }
    }
    title.to_vec()
}

/// Extracts the file size in bytes from an RSS description string (`FeedFile::ExtractSizeFromDescription`).
pub fn extract_size_from_description(description: &[u8]) -> i64 {
    let pos_opt = description
        .windows(4)
        .position(|w| w == b"Size")
        .or_else(|| description.windows(4).position(|w| w == b"size"));
    let Some(mut pos) = pos_opt else {
        return 0;
    };
    pos += 4;

    let mut size: f64 = 0.0;
    let mut found_size = false;
    let mut units: &[u8] = b"";

    while pos < description.len() {
        let ch = description[pos];
        if ch == b':' || ch.is_ascii_whitespace() {
            pos += 1;
            continue;
        }
        if !found_size {
            let tail = &description[pos..];
            let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
            let Some(val) = crate::dupecoord::parse_f64_prefix(&tail[..end]) else {
                break;
            };
            if !val.is_finite() {
                break;
            }
            found_size = true;
            size = val;
            while pos < description.len() && !description[pos].is_ascii_whitespace() {
                pos += 1;
            }
            pos += 1;
            continue;
        }

        if pos + 2 <= description.len() {
            units = &description[pos..pos + 2];
            break;
        }
        pos += 1;
    }

    match units {
        b"KB" => size *= 1024.0,
        b"MB" => size *= 1024.0 * 1024.0,
        b"GB" => size *= 1024.0 * 1024.0 * 1024.0,
        b"TB" => size *= 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => {}
    }

    size.round() as i64
}

/// Computes the length of `err_msg` after stripping trailing `'\n'`, `'\r'`, and `' '` bytes (`FeedFile::SAX_error`).
pub fn trim_sax_error_len(err_msg: &[u8]) -> usize {
    let mut end = err_msg.len();
    while end > 0 && matches!(err_msg[end - 1], b'\n' | b'\r' | b' ') {
        end -= 1;
    }
    end
}

/// A parsed RSS feed item produced by [`FeedFileParser`].
#[derive(Default, Debug, Clone)]
pub struct ParsedFeedItem {
    pub title: CString,
    pub filename: CString,
    pub url: CString,
    pub category: CString,
    pub description: CString,
    pub season: CString,
    pub episode: CString,
    pub has_season: bool,
    pub has_episode: bool,
    pub time: i64,
    pub size: i64,
    pub imdb_id: i32,
    pub rage_id: i32,
    pub tvdb_id: i32,
    pub tvmaze_id: i32,
    pub attr_names: Vec<CString>,
    pub attr_values: Vec<CString>,
    pub attr_name_ptrs: Vec<*const c_char>,
    pub attr_value_ptrs: Vec<*const c_char>,
}

/// End-to-end XML SAX state machine for `FeedFile` (`daemon/feed/FeedFile.cpp`).
#[derive(Default)]
pub struct FeedFileParser {
    items: Vec<ParsedFeedItem>,
    has_current_item: bool,
    current_element: Vec<u8>,
    tag_content: Vec<u8>,
}

impl FeedFileParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_element(&mut self, name: &[u8], atts: &[(&[u8], &[u8])]) {
        self.current_element.clear();
        self.current_element.extend_from_slice(name);
        self.tag_content.clear();

        if name == b"item" {
            self.items.push(ParsedFeedItem::default());
            self.has_current_item = true;
        } else if name == b"enclosure" && self.has_current_item {
            if let Some(item) = self.items.last_mut() {
                for &(attr_name, attr_val) in atts {
                    if attr_name == b"url" {
                        let decoded = truncate_at_nul(attr_val);
                        item.url = cstring_lossy(&decoded);
                    } else if attr_name == b"length" {
                        item.size = c_atoll_bytes(attr_val);
                    }
                    if item.size == 0 {
                        item.size = extract_size_from_description(item.description.as_bytes());
                    }
                }
            }
        } else if self.has_current_item
            && (name == b"newznab:attr" || name == b"nZEDb:attr")
            && atts.len() >= 2
            && atts[0].0 == b"name"
            && atts[1].0 == b"value"
        {
            let key = atts[0].1;
            let val = atts[1].1;
            if let Some(item) = self.items.last_mut() {
                let name_cstr = cstring_lossy(key);
                let val_cstr = cstring_lossy(val);
                item.attr_names.push(name_cstr);
                item.attr_values.push(val_cstr);

                match key {
                    b"size" if item.size == 0 => {
                        item.size = c_atoll_bytes(val);
                    }
                    b"imdb" => {
                        item.imdb_id = c_atoi_bytes(val);
                    }
                    b"rageid" => {
                        item.rage_id = c_atoi_bytes(val);
                    }
                    b"tvdbid" => {
                        item.tvdb_id = c_atoi_bytes(val);
                    }
                    b"tvmazeid" => {
                        item.tvmaze_id = c_atoi_bytes(val);
                    }
                    b"episode" => {
                        item.episode = cstring_lossy(val);
                        item.has_episode = true;
                    }
                    b"season" => {
                        item.season = cstring_lossy(val);
                        item.has_season = true;
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn end_element(&mut self, name: &[u8]) {
        self.tag_content = trim_string_bytes(&self.tag_content);

        if self.has_current_item {
            if let Some(item) = self.items.last_mut() {
                match name {
                    b"title" => {
                        let title_bytes = truncate_at_nul(&self.tag_content);
                        let filename_bytes = parse_feed_subject(&title_bytes);
                        item.title = cstring_lossy(&title_bytes);
                        item.filename = cstring_lossy(&filename_bytes);
                        self.tag_content.clear();
                    }
                    b"link" if item.url.as_bytes().is_empty() => {
                        item.url = cstring_lossy(&self.tag_content);
                        self.tag_content.clear();
                    }
                    b"category" => {
                        item.category = cstring_lossy(&self.tag_content);
                        self.tag_content.clear();
                    }
                    b"description" => {
                        let mut desc = truncate_at_nul(&self.tag_content);
                        crate::text::xml_strip_tags(&mut desc);
                        let len1 = crate::text::xml_decode(&mut desc, &xml_digit_lower);
                        desc.truncate(len1);
                        let len2 = crate::text::xml_remove_entities(&mut desc, &xml_entity_alpha);
                        desc.truncate(len2);
                        item.description = cstring_lossy(&desc);
                        self.tag_content.clear();
                    }
                    b"pubDate" => {
                        let date_cstr = cstring_lossy(&self.tag_content);
                        let unixtime = crate::util::parse_rfc822_date_time(&date_cstr) as isize;
                        if unixtime > 0 {
                            item.time = unixtime as i64;
                        }
                        self.tag_content.clear();
                    }
                    _ => {}
                }
            }
        }

        self.current_element.clear();
    }

    pub fn text(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        if self.current_element == b"category" {
            let trimmed = trim_string_bytes(chunk);
            self.tag_content.extend_from_slice(&trimmed);
        } else {
            self.tag_content.extend_from_slice(chunk);
        }
    }

    pub fn finalize_items(&mut self) -> &[ParsedFeedItem] {
        for item in &mut self.items {
            item.attr_name_ptrs = item.attr_names.iter().map(|s| s.as_ptr()).collect();
            item.attr_value_ptrs = item.attr_values.iter().map(|s| s.as_ptr()).collect();
        }
        &self.items
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

    pub fn parse_xml(xml: &[u8]) -> Option<Self> {
        use quick_xml::events::Event;
        if xml.is_empty() {
            return None;
        }
        let mut reader = quick_xml::Reader::from_reader(xml);
        let mut parser = Self::new();
        let mut buf = Vec::new();
        let mut saw_root = false;
        let mut root_closed = false;
        let mut depth: i32 = 0;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    if root_closed {
                        return None;
                    }
                    saw_root = true;
                    depth += 1;
                    let mut attrs = Vec::new();
                    for attr_res in e.attributes() {
                        let a = attr_res.ok()?;
                        let decoded = reader.decoder().decode(&a.value).ok()?;
                        let norm = Self::normalize_xml_attr_ws(&decoded);
                        let unescaped = quick_xml::escape::unescape(&norm).ok()?;
                        attrs.push((a.key.as_ref().to_vec(), unescaped.into_owned().into_bytes()));
                    }
                    let attr_refs: Vec<(&[u8], &[u8])> = attrs
                        .iter()
                        .map(|(k, v)| (k.as_slice(), v.as_slice()))
                        .collect();
                    parser.start_element(e.name().as_ref(), &attr_refs);
                }
                Ok(Event::Empty(e)) => {
                    if root_closed {
                        return None;
                    }
                    saw_root = true;
                    let mut attrs = Vec::new();
                    for attr_res in e.attributes() {
                        let a = attr_res.ok()?;
                        let decoded = reader.decoder().decode(&a.value).ok()?;
                        let norm = Self::normalize_xml_attr_ws(&decoded);
                        let unescaped = quick_xml::escape::unescape(&norm).ok()?;
                        attrs.push((a.key.as_ref().to_vec(), unescaped.into_owned().into_bytes()));
                    }
                    let attr_refs: Vec<(&[u8], &[u8])> = attrs
                        .iter()
                        .map(|(k, v)| (k.as_slice(), v.as_slice()))
                        .collect();
                    parser.start_element(e.name().as_ref(), &attr_refs);
                    parser.end_element(e.name().as_ref());
                    if depth == 0 {
                        root_closed = true;
                    }
                }
                Ok(Event::Text(t)) => {
                    let decoded = t.decode().ok()?;
                    let norm = Self::normalize_xml_eol(&decoded);
                    if depth == 0 {
                        let rest = norm.strip_prefix('\u{feff}').unwrap_or(&norm);
                        if !rest.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')) {
                            return None;
                        }
                    } else {
                        parser.text(norm.as_bytes());
                    }
                }
                Ok(Event::CData(c)) => {
                    if depth == 0 {
                        return None;
                    }
                    let decoded = c.decode().ok()?;
                    let norm = Self::normalize_xml_eol(&decoded);
                    parser.text(norm.as_bytes());
                }
                Ok(Event::GeneralRef(r)) => {
                    if depth == 0 {
                        return None;
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
                    }?;
                    parser.text(&entity);
                }
                Ok(Event::Decl(_)) => {
                    if saw_root {
                        return None;
                    }
                }
                Ok(Event::DocType(d)) => {
                    if saw_root || d.windows(8).any(|w| w.eq_ignore_ascii_case(b"<!entity")) {
                        return None;
                    }
                }
                Ok(Event::End(e)) => {
                    depth -= 1;
                    if depth < 0 {
                        return None;
                    }
                    parser.end_element(e.name().as_ref());
                    if depth == 0 {
                        root_closed = true;
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => return None,
                _ => {}
            }
            buf.clear();
        }
        (saw_root && root_closed && depth == 0).then_some(parser)
    }

    /// Reads an RSS/XML file from disk and parses it into a `FeedFileParser` (`FeedFile::Parse`).
    pub fn parse_file(path: &std::path::Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        Self::parse_xml(&bytes)
    }
}

// ============================================================================
// 3. NzbInfoCreator & FeedCoordinator (`daemon/feed/FeedCoordinator.cpp`)
// ============================================================================

/// Normalizes a feed item filename to end with `.nzb` (`NzbInfoCreator::Create`).
pub fn normalize_feed_nzb_name(filename: &[u8]) -> Vec<u8> {
    let mut stem = filename;
    if let Some(dot) = filename.iter().rposition(|&b| b == b'.') {
        if filename[dot..].eq_ignore_ascii_case(b".nzb") {
            stem = &filename[..dot];
        }
    }
    if stem.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(stem.len() + 4);
    out.extend_from_slice(stem);
    out.extend_from_slice(b".nzb");
    out
}

/// Resolves the effective category for a feed item (`FeedCoordinator::ResolveCategory`).
///
/// `category_source`:
/// - `0`: `CategorySource::Auto`
/// - `1`: `CategorySource::NZBFile`
/// - `2`: `CategorySource::FeedFile`
pub fn resolve_category<'a>(
    feed_cat: &'a [u8],
    category_source: i32,
    item_cat: &'a [u8],
) -> &'a [u8] {
    if !feed_cat.is_empty() {
        return feed_cat;
    }
    // CategorySource::Auto (0) or CategorySource::NZBFile (1)
    if category_source == 0 || category_source == 1 {
        item_cat
    } else {
        b""
    }
}

/// Feed-level inputs for `create_nzb_info` (`NzbInfoCreator::Create`).
#[derive(Clone, Copy, Debug, Default)]
pub struct FeedCreationInput<'a> {
    pub id: i32,
    pub category: &'a [u8],
    pub category_source: i32,
}

/// Item-level inputs for `create_nzb_info` (`NzbInfoCreator::Create`).
#[derive(Clone, Copy, Debug, Default)]
pub struct FeedItemCreationInput<'a> {
    pub url: &'a [u8],
    pub filename: &'a [u8],
    pub category: &'a [u8],
    pub add_category: &'a [u8],
    pub size: i64,
    pub time: i64,
    pub priority: i32,
    pub pause_nzb: bool,
    pub dupe_key: &'a [u8],
    pub dupe_score: i32,
    pub dupe_mode: i32,
}

/// Output fields of `NzbInfoCreator::Create` (`daemon/feed/FeedCoordinator.cpp`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatedNzbInfo {
    pub kind: i32,
    pub feed_id: i32,
    pub url: Vec<u8>,
    pub filename: Vec<u8>,
    pub category: Vec<u8>,
    pub priority: i32,
    pub add_url_paused: bool,
    pub dupe_key: Vec<u8>,
    pub dupe_score: i32,
    pub dupe_mode: i32,
    pub size: i64,
    pub min_time: i64,
    pub max_time: i64,
}

/// End-to-end implementation of `NzbInfoCreator::Create` + `NzbInfoCreator::ApplyCategory`
/// (`daemon/feed/FeedCoordinator.cpp`): normalizes and sanitizes the `.nzb` filename,
/// applies `add_category` override or `resolve_category`, and populates all `NzbInfo` fields.
pub fn create_nzb_info(
    feed: &FeedCreationInput<'_>,
    item: &FeedItemCreationInput<'_>,
) -> CreatedNzbInfo {
    let norm = normalize_feed_nzb_name(item.filename);
    let filename = if norm.is_empty() {
        Vec::new()
    } else {
        let truncated = &norm[..norm.len().min(1023)];
        crate::paths::make_valid_filename(truncated, false)
    };
    let category = if !item.add_category.is_empty() {
        item.add_category.to_vec()
    } else {
        resolve_category(feed.category, feed.category_source, item.category).to_vec()
    };
    CreatedNzbInfo {
        kind: 1, // NzbInfo::nkUrl
        feed_id: feed.id,
        url: item.url.to_vec(),
        filename,
        category,
        priority: item.priority,
        add_url_paused: item.pause_nzb,
        dupe_key: item.dupe_key.to_vec(),
        dupe_score: item.dupe_score,
        dupe_mode: item.dupe_mode,
        size: item.size,
        min_time: item.time,
        max_time: item.time,
    }
}

/// Formats the `DupeCoordinator::EDupeStatus` bitmask (`FeedCoordinator::FilterHelper::CalcDupeStatus`).
pub fn format_dupe_status_mask(dupe_status: i32) -> Vec<u8> {
    const TABLE: [(i32, &[u8]); 5] = [
        (1, b"QUEUED"),
        (2, b"DOWNLOADING"),
        (4, b"SUCCESS"),
        (8, b"WARNING"),
        (16, b"FAILURE"),
    ];
    let mut out = Vec::new();
    for (bit, name) in TABLE {
        if (dupe_status & bit) != 0 {
            if !out.is_empty() {
                out.push(b',');
            }
            out.extend_from_slice(name);
        }
    }
    out
}

/// Evaluates whether a feed download should start (`FeedCoordinator::Run`).
/// Returns `(should_start_download, set_force)`.
pub fn should_start_feed(
    interval_min: i32,
    next_update: i64,
    current_time: i64,
    fetch: bool,
    status: i32,
    fs_running: i32,
) -> (bool, bool) {
    let interval_sec = (interval_min as i64).saturating_mul(60);
    let due = (interval_min > 0
        && (next_update == 0
            || current_time >= next_update
            || current_time < next_update - interval_sec))
        || fetch;
    if due && status != fs_running {
        (true, false)
    } else if fetch {
        (false, true)
    } else {
        (false, false)
    }
}

/// Calculates the next update interval (`FeedCoordinator::SchedulerNextUpdate`).
/// Returns `(interval_sec, new_last_interval_sec)`.
pub fn calc_feed_next_interval(
    success: bool,
    interval_min: i32,
    last_interval_sec: i32,
) -> (i32, i32) {
    let max_interval = interval_min.saturating_mul(60);
    if success {
        (max_interval, 0)
    } else {
        let mut interval = last_interval_sec.saturating_mul(2);
        interval = interval.max(60);
        interval = interval.min(max_interval);
        (interval, interval)
    }
}

/// Formats the temporary feed download filename (`FeedCoordinator::StartFeedDownload`).
pub fn format_temp_filename(
    temp_dir: &[u8],
    sep: u8,
    id: i32,
    preview: bool,
    cur_time: i32,
    rand_val: i32,
    preview_num: i32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(temp_dir.len() + 48);
    out.extend_from_slice(temp_dir);
    out.push(sep);
    if id > 0 && !preview {
        out.extend_from_slice(format!("feed-{id}.tmp").as_bytes());
    } else {
        out.extend_from_slice(format!("feed-{cur_time}-{rand_val}-{preview_num}.tmp").as_bytes());
    }
    out
}

/// Classifies an accepted feed item against history and backlog settings (`FeedCoordinator::ProcessFeed`).
/// Returns `(history_status, should_add)` where:
/// - `0`: `hsUnknown`
/// - `1`: `hsBacklog`
/// - `2`: `hsFetched`
pub fn classify_item_history(first_fetch: bool, backlog: bool, has_history: bool) -> (i32, bool) {
    let status = if first_fetch && backlog {
        1 // hsBacklog
    } else if !has_history {
        2 // hsFetched
    } else {
        0 // hsUnknown
    };
    (status, status == 2)
}

/// Resolves the preview status for a feed item (`FeedCoordinator::PreviewFeed`).
/// - `1`: `isBacklog`
/// - `2`: `isFetched`
/// - `3`: `isNew`
pub fn resolve_preview_item_status(
    has_history: bool,
    history_status: i32,
    first_fetch: bool,
    backlog: bool,
) -> i32 {
    if has_history {
        history_status
    } else if first_fetch && backlog {
        1 // FeedItemInfo::isBacklog
    } else {
        3 // FeedItemInfo::isNew
    }
}

/// Computes the history retention border date (`FeedCoordinator::CleanupHistory`)
/// from the current time, feed `last_update` timestamps, and `feed_history_days`.
pub fn calc_history_border_date(
    cur_time: i64,
    last_updates: &[i64],
    feed_history_days: i32,
) -> i64 {
    let mut oldest = cur_time;
    for &ts in last_updates {
        if ts < oldest {
            oldest = ts;
        }
    }
    oldest.wrapping_sub(i64::from(feed_history_days).wrapping_mul(60 * 60 * 24))
}

/// Evaluates whether a feed history entry is older than `border_date` (`FeedCoordinator::CleanupHistory`),
/// returning `Some(url_len_without_query)` when it should be deleted, or `None` to keep it.
pub fn should_delete_history_entry(
    last_seen: i64,
    border_date: i64,
    url: &[u8],
) -> Option<usize> {
    if last_seen < border_date {
        Some(url.iter().position(|&b| b == b'?').unwrap_or(url.len()))
    } else {
        None
    }
}

/// Evaluates whether a cached feed preview item has expired (`FeedCoordinator::CleanupCache`).
pub fn is_feed_cache_expired(last_usage: i64, cache_time_sec: i32, cur_time: i64) -> bool {
    last_usage.saturating_add(cache_time_sec as i64) < cur_time || last_usage > cur_time
}

/// Evaluates whether an active feed download has exceeded `url_timeout + 10` seconds
/// (`FeedCoordinator::ResetHangingDownloads`).
pub fn is_feed_download_hanging(
    cur_time: i64,
    last_update_time: i64,
    url_timeout: i32,
    status: i32,
    running_status: i32,
) -> bool {
    url_timeout != 0
        && status == running_status
        && cur_time.wrapping_sub(last_update_time) > i64::from(url_timeout.wrapping_add(10))
}

/// Evaluates whether an existing configured feed matches the preview feed's URL and filter
/// and has already been updated at least once (`FeedCoordinator::PreviewFeed`).
pub fn is_feed_url_and_filter_updated(
    existing_url: &[u8],
    preview_url: &[u8],
    existing_filter: &[u8],
    preview_filter: &[u8],
    existing_last_update: i64,
) -> bool {
    existing_last_update > 0 && existing_url == preview_url && existing_filter == preview_filter
}

// ============================================================================
// 4. C ABI Boundary (`nzbget_rs_feed_*`)
// ============================================================================

/// View of a parsed RSS feed item returned across FFI to `FeedFile::Parse`.
#[repr(C)]
pub struct NzbgetRsParsedFeedItemView {
    pub title: *const c_char,
    pub filename: *const c_char,
    pub url: *const c_char,
    pub category: *const c_char,
    pub description: *const c_char,
    pub season: *const c_char,
    pub episode: *const c_char,
    pub has_season: c_int,
    pub has_episode: c_int,
    pub time: i64,
    pub size: i64,
    pub imdb_id: c_int,
    pub rage_id: c_int,
    pub tvdb_id: c_int,
    pub tvmaze_id: c_int,
    pub attr_names: *const *const c_char,
    pub attr_values: *const *const c_char,
    pub attr_count: usize,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_info_init(
    id: c_int,
    name: *const c_char,
    filter: *const c_char,
    out_default_name: *mut crate::ffi::RsBuf,
    out_filter_hash: *mut u32,
) {
    let name_bytes = crate::diskstate_format::cstr_to_bytes(name);
    let filter_bytes = crate::diskstate_format::cstr_to_bytes(filter);
    let (default_name, hash) = feed_info_init(id, name_bytes, filter_bytes);
    if !out_default_name.is_null() {
        *out_default_name = match default_name {
            Some(bytes) => crate::ffi::into_buf(bytes),
            None => std::mem::zeroed(),
        };
    }
    if !out_filter_hash.is_null() {
        *out_filter_hash = hash;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_parse_prefixed_int(value: *const c_char) -> c_int {
    parse_prefixed_int(crate::diskstate_format::cstr_to_bytes(value))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_has_any_series_id(
    series: *const c_char,
    rage_id: *const c_char,
    tvdb_id: *const c_char,
    tvmaze_id: *const c_char,
    item_rage_id: c_int,
    item_tvdb_id: c_int,
    item_tvmaze_id: c_int,
) -> c_int {
    i32::from(has_any_series_id(
        crate::diskstate_format::cstr_to_bytes(series),
        crate::diskstate_format::cstr_to_bytes(rage_id),
        crate::diskstate_format::cstr_to_bytes(tvdb_id),
        crate::diskstate_format::cstr_to_bytes(tvmaze_id),
        item_rage_id,
        item_tvdb_id,
        item_tvmaze_id,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_build_dupe_key(
    rage_id: *const c_char,
    tvdb_id: *const c_char,
    tvmaze_id: *const c_char,
    series: *const c_char,
    item_rage_id: c_int,
    item_tvdb_id: c_int,
    item_tvmaze_id: c_int,
    item_imdb_id: c_int,
    season_num: c_int,
    episode_num: c_int,
    season: *const c_char,
    episode: *const c_char,
) -> crate::ffi::RsBuf {
    crate::ffi::into_buf(build_feed_dupe_key(
        crate::diskstate_format::cstr_to_bytes(rage_id),
        crate::diskstate_format::cstr_to_bytes(tvdb_id),
        crate::diskstate_format::cstr_to_bytes(tvmaze_id),
        crate::diskstate_format::cstr_to_bytes(series),
        item_rage_id,
        item_tvdb_id,
        item_tvmaze_id,
        item_imdb_id,
        season_num,
        episode_num,
        crate::diskstate_format::cstr_to_bytes(season),
        crate::diskstate_format::cstr_to_bytes(episode),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_parse_season_episode(
    title: *const c_char,
    out_season: *mut crate::ffi::RsBuf,
    out_episode: *mut crate::ffi::RsBuf,
) -> c_int {
    let title_slice = crate::diskstate_format::cstr_to_bytes(title);
    let Some((season, episode)) = parse_season_episode(title_slice) else {
        return 0;
    };
    if !out_season.is_null() {
        *out_season = crate::ffi::into_buf(season);
    }
    if !out_episode.is_null() {
        *out_episode = crate::ffi::into_buf(episode);
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_file_parser_parse_file(
    filename: *const c_char,
) -> *mut FeedFileParser {
    let bytes = crate::diskstate_format::cstr_to_bytes(filename);
    if bytes.is_empty() {
        return std::ptr::null_mut();
    }
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStrExt;
        std::path::Path::new(std::ffi::OsStr::from_bytes(bytes))
    };
    #[cfg(not(unix))]
    let path_buf = std::path::PathBuf::from(String::from_utf8_lossy(bytes).into_owned());
    #[cfg(not(unix))]
    let path = path_buf.as_path();

    match FeedFileParser::parse_file(path) {
        Some(parser) => Box::into_raw(Box::new(parser)),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_file_parser_free(parser: *mut FeedFileParser) {
    if !parser.is_null() {
        drop(Box::from_raw(parser));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_file_parser_finalize(parser: *mut FeedFileParser) -> usize {
    if parser.is_null() {
        return 0;
    }
    (*parser).finalize_items().len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_file_parser_get_item(
    parser: *const FeedFileParser,
    index: usize,
    out_view: *mut NzbgetRsParsedFeedItemView,
) -> c_int {
    if parser.is_null() || out_view.is_null() {
        return 0;
    }
    let parser_ref = &*parser;
    let Some(item) = parser_ref.items.get(index) else {
        return 0;
    };
    *out_view = NzbgetRsParsedFeedItemView {
        title: item.title.as_ptr(),
        filename: item.filename.as_ptr(),
        url: item.url.as_ptr(),
        category: item.category.as_ptr(),
        description: item.description.as_ptr(),
        season: item.season.as_ptr(),
        episode: item.episode.as_ptr(),
        has_season: i32::from(item.has_season),
        has_episode: i32::from(item.has_episode),
        time: item.time,
        size: item.size,
        imdb_id: item.imdb_id,
        rage_id: item.rage_id,
        tvdb_id: item.tvdb_id,
        tvmaze_id: item.tvmaze_id,
        attr_names: item.attr_name_ptrs.as_ptr(),
        attr_values: item.attr_value_ptrs.as_ptr(),
        attr_count: item.attr_name_ptrs.len(),
    };
    1
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_create_nzb_info(
    feed_id: c_int,
    feed_cat: *const c_char,
    category_source: c_int,
    item_url: *const c_char,
    item_filename: *const c_char,
    item_cat: *const c_char,
    item_add_cat: *const c_char,
    item_size: i64,
    item_time: i64,
    item_priority: c_int,
    item_pause_nzb: c_int,
    item_dupe_key: *const c_char,
    item_dupe_score: c_int,
    item_dupe_mode: c_int,
    out_filename: *mut crate::ffi::RsBuf,
    out_category: *mut crate::ffi::RsBuf,
) {
    let feed = FeedCreationInput {
        id: feed_id,
        category: crate::diskstate_format::cstr_to_bytes(feed_cat),
        category_source,
    };
    let item = FeedItemCreationInput {
        url: crate::diskstate_format::cstr_to_bytes(item_url),
        filename: crate::diskstate_format::cstr_to_bytes(item_filename),
        category: crate::diskstate_format::cstr_to_bytes(item_cat),
        add_category: crate::diskstate_format::cstr_to_bytes(item_add_cat),
        size: item_size,
        time: item_time,
        priority: item_priority,
        pause_nzb: item_pause_nzb != 0,
        dupe_key: crate::diskstate_format::cstr_to_bytes(item_dupe_key),
        dupe_score: item_dupe_score,
        dupe_mode: item_dupe_mode,
    };
    let created = create_nzb_info(&feed, &item);
    if !out_filename.is_null() {
        *out_filename = crate::ffi::into_buf(created.filename);
    }
    if !out_category.is_null() {
        *out_category = crate::ffi::into_buf(created.category);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_resolve_category(
    feed_cat: *const c_char,
    category_source: c_int,
    item_cat: *const c_char,
) -> *const c_char {
    let f_slice = crate::diskstate_format::cstr_to_bytes(feed_cat);
    let i_slice = crate::diskstate_format::cstr_to_bytes(item_cat);
    let res = resolve_category(f_slice, category_source, i_slice);
    if res.is_empty() {
        c"".as_ptr()
    } else if std::ptr::eq(res.as_ptr(), f_slice.as_ptr()) {
        feed_cat
    } else {
        item_cat
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_format_dupe_status_mask(
    dupe_status: c_int,
) -> crate::ffi::RsBuf {
    crate::ffi::into_buf(format_dupe_status_mask(dupe_status))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_should_start(
    interval_min: c_int,
    next_update: i64,
    current_time: i64,
    fetch: c_int,
    status: c_int,
    fs_running: c_int,
    out_set_force: *mut c_int,
) -> c_int {
    let (start, set_force) = should_start_feed(
        interval_min,
        next_update,
        current_time,
        fetch != 0,
        status,
        fs_running,
    );
    if !out_set_force.is_null() {
        *out_set_force = i32::from(set_force);
    }
    i32::from(start)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_calc_next_interval(
    success: c_int,
    interval_min: c_int,
    last_interval_sec: c_int,
    out_new_last_interval_sec: *mut c_int,
) -> c_int {
    let (interval_sec, new_last) =
        calc_feed_next_interval(success != 0, interval_min, last_interval_sec);
    if !out_new_last_interval_sec.is_null() {
        *out_new_last_interval_sec = new_last;
    }
    interval_sec
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_format_temp_filename(
    temp_dir: *const c_char,
    sep: c_char,
    id: c_int,
    preview: c_int,
    cur_time: c_int,
    rand_val: c_int,
    preview_num: c_int,
) -> crate::ffi::RsBuf {
    crate::ffi::into_buf(format_temp_filename(
        crate::diskstate_format::cstr_to_bytes(temp_dir),
        sep as u8,
        id,
        preview != 0,
        cur_time,
        rand_val,
        preview_num,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_classify_item_history(
    first_fetch: c_int,
    backlog: c_int,
    has_history: c_int,
    out_should_add: *mut c_int,
) -> c_int {
    let (status, should_add) =
        classify_item_history(first_fetch != 0, backlog != 0, has_history != 0);
    if !out_should_add.is_null() {
        *out_should_add = i32::from(should_add);
    }
    status
}

#[no_mangle]
pub extern "C" fn nzbget_rs_feed_resolve_preview_item_status(
    has_history: c_int,
    history_status: c_int,
    first_fetch: c_int,
    backlog: c_int,
) -> c_int {
    resolve_preview_item_status(
        has_history != 0,
        history_status,
        first_fetch != 0,
        backlog != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_calc_history_border_date(
    cur_time: i64,
    last_updates: *const i64,
    last_updates_len: usize,
    feed_history_days: c_int,
) -> i64 {
    let slice = if last_updates.is_null() || last_updates_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(last_updates, last_updates_len)
    };
    calc_history_border_date(cur_time, slice, feed_history_days)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_should_delete_history_entry(
    last_seen: i64,
    border_date: i64,
    url: *const c_char,
    out_url_len_without_query: *mut usize,
) -> c_int {
    match should_delete_history_entry(
        last_seen,
        border_date,
        crate::diskstate_format::cstr_to_bytes(url),
    ) {
        Some(len) => {
            if !out_url_len_without_query.is_null() {
                *out_url_len_without_query = len;
            }
            1
        }
        None => {
            if !out_url_len_without_query.is_null() {
                *out_url_len_without_query = 0;
            }
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_feed_is_cache_expired(
    last_usage: i64,
    cache_time_sec: c_int,
    cur_time: i64,
) -> c_int {
    i32::from(is_feed_cache_expired(last_usage, cache_time_sec, cur_time))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feed_info_and_dupe_key() {
        let (def_name, hash) = feed_info_init(3, b"", b"title:test");
        assert_eq!(def_name.as_deref(), Some(&b"Feed3"[..]));
        assert_ne!(hash, 0);

        assert_eq!(parse_prefixed_int(b"S03"), 3);
        assert_eq!(parse_prefixed_int(b"E69"), 69);
        assert_eq!(parse_prefixed_int(b"42"), 42);

        let (season, episode) = format_season_episode(3, b"02-e03");
        assert_eq!(season, b"S03");
        assert_eq!(episode, b"E02E03");

        assert!(has_any_series_id(b"", b"", b"", b"", 0, 439270, 0));
        assert_eq!(
            build_feed_dupe_key(
                b"", b"", b"", b"", 0, 439270, 0, 28369634, 2, 69, b"S02", b"E69"
            ),
            b"tvdbid=439270-S02-E69"
        );
    }

    #[test]
    fn test_feed_file_parser_end_to_end() {
        let mut parser = FeedFileParser::new();
        parser.start_element(b"item", &[]);
        parser.start_element(b"title", &[]);
        parser.text(b"Post [1/10] - \"My.Release.par2\" yEnc (1/1)");
        parser.end_element(b"title");

        parser.start_element(b"category", &[]);
        parser.text(b"  Movies ");
        parser.text(b">");
        parser.text(b" HD  ");
        parser.end_element(b"category");

        parser.start_element(b"description", &[]);
        parser.text(b"<b>Total Size</b> : 299.6 MB | MultiUp");
        parser.end_element(b"description");

        parser.start_element(b"enclosure", &[(b"url", b"http://x/a.nzb&k=1")]);
        parser.end_element(b"enclosure");

        parser.start_element(
            b"newznab:attr",
            &[(b"name", b"tvdbid"), (b"value", b"33877")],
        );
        parser.end_element(b"newznab:attr");
        parser.end_element(b"item");

        let items = parser.finalize_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].filename.as_bytes(), b"My.Release");
        assert_eq!(items[0].category.as_bytes(), b"Movies>HD");
        assert_eq!(items[0].url.as_bytes(), b"http://x/a.nzb&k=1");
        assert_eq!(items[0].tvdb_id, 33877);
        assert_eq!(items[0].size, (299.6f64 * 1024.0 * 1024.0).round() as i64);
    }

    #[test]
    fn test_feed_coordinator_logic() {
        assert_eq!(normalize_feed_nzb_name(b"Movie.NZB"), b"Movie.nzb");
        assert_eq!(
            format_dupe_status_mask(1 | 4 | 16),
            b"QUEUED,SUCCESS,FAILURE"
        );
        assert_eq!(resolve_category(b"", 0, b"Movies"), b"Movies");
        assert_eq!(resolve_category(b"", 2, b"Movies"), b"");
        assert_eq!(should_start_feed(15, 1000, 1200, false, 0, 1), (true, false));
        assert_eq!(calc_feed_next_interval(false, 15, 60), (120, 120));
        assert_eq!(classify_item_history(true, true, false), (1, false));
        assert_eq!(classify_item_history(false, true, false), (2, true));
        assert_eq!(
            calc_history_border_date(100_000, &[95_000, 90_000], 1),
            90_000 - 86_400
        );
        assert_eq!(
            should_delete_history_entry(100, 200, b"https://idx.test/get?id=1&apikey=secret"),
            Some(20)
        );
        assert_eq!(c_atoll_bytes(b"9223372036854775808"), i64::MAX);
        assert_eq!(c_atoll_bytes(b"-9223372036854775809"), i64::MIN);
        assert_eq!(parse_prefixed_int(b"S4294967296"), i32::MAX);
        assert_eq!(parse_prefixed_int(b"E-4294967296"), i32::MIN);
    }

    #[test]
    fn feed_info_and_nzb_info_creator_ports_from_cpp() {
        // Ported from tests/feed/FeedInfo.cpp
        let ep1 = build_feed_dupe_key(
            b"", b"", b"", b"", 0, 439270, 0, 28369634, 2, 69, b"S02", b"E69",
        );
        let ep2 = build_feed_dupe_key(
            b"", b"", b"", b"", 0, 439270, 0, 28369634, 2, 71, b"S02", b"E71",
        );
        assert_eq!(ep1, b"tvdbid=439270-S02-E69");
        assert_eq!(ep2, b"tvdbid=439270-S02-E71");
        assert_ne!(ep1, ep2);

        let movie = build_feed_dupe_key(b"", b"", b"", b"", 0, 0, 0, 28369634, 0, 0, b"", b"");
        assert_eq!(movie, b"imdb=28369634");

        let ep_no_series = build_feed_dupe_key(
            b"", b"", b"", b"", 0, 0, 0, 28369634, 2, 69, b"S02", b"E69",
        );
        assert_eq!(ep_no_series, b"imdb=28369634");

        // Series identifier priority: series > rageid > tvdbid > tvmazeid
        assert_eq!(
            build_feed_dupe_key(
                b"",
                b"",
                b"",
                b"Landarztpraxis",
                12345,
                439270,
                123456,
                28369634,
                2,
                69,
                b"S02",
                b"E69"
            ),
            b"series=Landarztpraxis-S02-E69"
        );
        assert_eq!(
            build_feed_dupe_key(
                b"", b"", b"", b"", 12345, 439270, 123456, 28369634, 2, 69, b"S02", b"E69"
            ),
            b"rageid=12345-S02-E69"
        );
        assert_eq!(
            build_feed_dupe_key(
                b"", b"", b"", b"", 0, 439270, 123456, 28369634, 2, 69, b"S02", b"E69"
            ),
            b"tvdbid=439270-S02-E69"
        );
        assert_eq!(
            build_feed_dupe_key(
                b"", b"", b"", b"", 0, 0, 123456, 28369634, 2, 69, b"S02", b"E69"
            ),
            b"tvmazeid=123456-S02-E69"
        );
        assert_eq!(
            extract_size_from_description(b"Size: 1e3 MB"),
            1000 * 1024 * 1024
        );

        // Ported from tests/feed/NzbInfoCreator.cpp: full create_nzb_info tests
        let default_feed = FeedCreationInput {
            id: 42,
            category: b"Movies",
            category_source: 0, // Auto
        };
        let default_item = FeedItemCreationInput {
            url: b"https://example.com/file.nzb",
            filename: b"TestFile",
            category: b"",
            add_category: b"",
            size: 12345,
            time: 1000,
            priority: 5,
            pause_nzb: true,
            dupe_key: b"dupe-key",
            dupe_score: 7,
            dupe_mode: 2, // dmAll
        };
        let created_default = create_nzb_info(&default_feed, &default_item);
        assert_eq!(created_default.kind, 1); // nkUrl
        assert_eq!(created_default.feed_id, 42);
        assert_eq!(created_default.url, b"https://example.com/file.nzb");
        assert_eq!(created_default.filename, b"TestFile.nzb");
        assert_eq!(created_default.category, b"Movies");
        assert_eq!(created_default.priority, 5);
        assert!(created_default.add_url_paused);
        assert_eq!(created_default.dupe_key, b"dupe-key");
        assert_eq!(created_default.dupe_score, 7);
        assert_eq!(created_default.dupe_mode, 2);
        assert_eq!(created_default.size, 12345);
        assert_eq!(created_default.min_time, 1000);
        assert_eq!(created_default.max_time, 1000);

        // FilenameExtensionHandlingTest via create_nzb_info
        for (raw_name, expected_name) in [
            (&b"Alpha.Beta"[..], &b"Alpha.Beta.nzb"[..]),
            (&b"Gamma.nZb"[..], &b"Gamma.nzb"[..]),
            (&b""[..], &b""[..]),
            (&b"Bad:Name?.nzb"[..], &b"Bad_Name_.nzb"[..]),
        ] {
            let mut it = default_item;
            it.filename = raw_name;
            assert_eq!(create_nzb_info(&default_feed, &it).filename, expected_name);
        }

        // ApplyCategoryOverridesTest + ApplyCategoryFeedFileSourceTest + ApplyCategoryNzbFileSourceTest + ApplyCategoryAutoSourceTest
        for (feed_cat, source, item_cat, add_cat, expected_cat) in [
            (&b"FeedCategory"[..], 0, &b"ItemCategory"[..], &b"OverrideCategory"[..], &b"OverrideCategory"[..]),
            (&b"FeedCategory"[..], 2, &b"ItemCategory"[..], &b""[..], &b"FeedCategory"[..]),
            (&b""[..], 2, &b"ItemCategory"[..], &b""[..], &b""[..]),
            (&b"   "[..], 2, &b"   "[..], &b""[..], &b"   "[..]),
            (&b"FeedCategory"[..], 1, &b"ItemCategory"[..], &b""[..], &b"FeedCategory"[..]),
            (&b""[..], 1, &b"ItemCategory"[..], &b""[..], &b"ItemCategory"[..]),
            (&b""[..], 1, &b""[..], &b""[..], &b""[..]),
            (&b"FeedCategory"[..], 0, &b"ItemCategory"[..], &b""[..], &b"FeedCategory"[..]),
            (&b""[..], 0, &b"ItemCategory"[..], &b""[..], &b"ItemCategory"[..]),
            (&b""[..], 0, &b""[..], &b""[..], &b""[..]),
        ] {
            let f = FeedCreationInput {
                id: 1,
                category: feed_cat,
                category_source: source,
            };
            let mut it = default_item;
            it.category = item_cat;
            it.add_category = add_cat;
            assert_eq!(create_nzb_info(&f, &it).category, expected_cat);
        }

        // CreateNzbInfoAllPropertiesTest
        let all_feed = FeedCreationInput {
            id: 42,
            category: b"Movies>HD",
            category_source: 0,
        };
        let all_item = FeedItemCreationInput {
            url: b"https://example.com/movie.nzb",
            filename: b"Movie.Title.2024",
            category: b"Movies>HD",
            add_category: b"OverrideCategory",
            size: 987_654_321,
            time: 2000,
            priority: 15,
            pause_nzb: false,
            dupe_key: b"movie-key-123",
            dupe_score: 10,
            dupe_mode: 0, // dmScore
        };
        let created_all = create_nzb_info(&all_feed, &all_item);
        assert_eq!(created_all.kind, 1);
        assert_eq!(created_all.feed_id, 42);
        assert_eq!(created_all.url, b"https://example.com/movie.nzb");
        assert_eq!(created_all.filename, b"Movie.Title.2024.nzb");
        assert_eq!(created_all.category, b"OverrideCategory");
        assert_eq!(created_all.priority, 15);
        assert!(!created_all.add_url_paused);
        assert_eq!(created_all.dupe_key, b"movie-key-123");
        assert_eq!(created_all.dupe_score, 10);
        assert_eq!(created_all.dupe_mode, 0);
        assert_eq!(created_all.size, 987_654_321);
        assert_eq!(created_all.min_time, 2000);
        assert_eq!(created_all.max_time, 2000);

        // CategorySource: 0 = Auto, 1 = NZBFile, 2 = FeedFile
        assert_eq!(resolve_category(b"FeedCat", 1, b"ItemCat"), b"FeedCat");
        assert_eq!(resolve_category(b"FeedCat", 0, b"ItemCat"), b"FeedCat");
        assert_eq!(resolve_category(b"FeedCat", 2, b"ItemCat"), b"FeedCat");
        assert_eq!(resolve_category(b"FeedCat", 1, b""), b"FeedCat");
        assert_eq!(resolve_category(b"FeedCat", 0, b""), b"FeedCat");
        assert_eq!(resolve_category(b"", 1, b"ItemCat"), b"ItemCat");
        assert_eq!(resolve_category(b"", 0, b"ItemCat"), b"ItemCat");
        assert_eq!(resolve_category(b"", 1, b""), b"");
        assert_eq!(resolve_category(b"", 0, b""), b"");
        assert_eq!(resolve_category(b"", 2, b"ItemCat"), b"");
        assert_eq!(resolve_category(b"   ", 2, b"   "), b"   ");
    }

    #[test]
    fn feed_file_tests_port_from_cpp() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/testdata/feed");

        // FeedFileTest (feed.xml via parse_file)
        let mut p1 = FeedFileParser::parse_file(&root.join("feed.xml")).unwrap();
        let items1 = p1.finalize_items();
        assert_eq!(items1.len(), 6);
        assert_eq!(items1[0].title.as_bytes(), b"Crows.And.Sparrows");
        assert_eq!(items1[0].filename.as_bytes(), b"Crows.And.Sparrows");
        assert_eq!(items1[0].category.as_bytes(), b"Movies>HD");
        assert_eq!(items1[0].time, 1701668883);
        assert_eq!(items1[0].episode.as_bytes(), b"1");
        assert_eq!(parse_prefixed_int(items1[0].episode.as_bytes()), 1);
        assert_eq!(items1[0].season.as_bytes(), b"S03");
        assert_eq!(parse_prefixed_int(items1[0].season.as_bytes()), 3);
        assert_eq!(items1[0].tvmaze_id, 33877);
        assert_eq!(items1[0].tvdb_id, 33877);
        assert_eq!(items1[0].rage_id, 33877);
        assert_eq!(items1[0].imdb_id, 42054);
        assert_eq!(
            items1[0].url.as_bytes(),
            b"https://indexer.com/getnzb/nzb.nzb"
        );
        assert_eq!(
            items1[0].description.as_bytes(),
            b"   Title:     Test.Title         Added to index:     09/05/2025 14:05:12        Total Size         : 200.0 B         Weblink:     N/A"
        );
        assert_eq!(items1[0].size, 7445312955);

        assert_eq!(items1[1].title.as_bytes(), b"Deep.Path.Item");
        assert_eq!(items1[1].category.as_bytes(), b"Movies>HD>4K");
        assert_eq!(items1[2].title.as_bytes(), b"Spaced.Category.Item");
        assert_eq!(items1[2].category.as_bytes(), b"Movies>HD");
        assert_eq!(items1[3].title.as_bytes(), b"Empty.Category.Item");
        assert_eq!(items1[3].category.as_bytes(), b"");
        assert_eq!(items1[4].title.as_bytes(), b"Preserved.Spaces.Item");
        assert_eq!(items1[4].category.as_bytes(), b"My Movies");
        assert_eq!(items1[5].title.as_bytes(), b"Literal.Gt.Item");
        assert_eq!(items1[5].category.as_bytes(), b"Movies > HD");

        // FeedFile2Test (feed2.xml)
        let xml2 = std::fs::read(root.join("feed2.xml")).unwrap();
        let mut p2 = FeedFileParser::parse_xml(&xml2).unwrap();
        let items2 = p2.finalize_items();
        let last2 = items2.last().unwrap();
        let expected_title2 = b"[Judas] Kimi to Boku no Saigo no Senjou, Aruiwa Sekai ga Hajimaru Seisen (Our Last Crusade or the Rise of a New World) - S02E08 [1080p][HEVC x265 10bit][Multi-Subs] (Weekly)";
        assert_eq!(last2.category.as_bytes(), b"");
        assert_eq!(last2.time, 1748526661);
        assert_eq!(last2.episode.as_bytes(), b"");
        assert_eq!(last2.season.as_bytes(), b"");
        assert_eq!(last2.tvmaze_id, 0);
        assert_eq!(last2.tvdb_id, 0);
        assert_eq!(last2.rage_id, 0);
        assert_eq!(last2.imdb_id, 0);
        assert_eq!(
            last2.url.as_bytes(),
            b"http://storage.animetosho.org/nzbs/000a9b44/%5BJudas%5D%20Kimisen%20-%20S02E08.nzb"
        );
        assert_eq!(
            last2.description.as_bytes(),
            b"        Total Size         : 299.6 MB              |                                                    MultiUp    "
        );
        assert_eq!(last2.filename.as_bytes(), expected_title2);
        assert_eq!(last2.title.as_bytes(), expected_title2);
        assert_eq!(last2.size, (299.6f64 * 1024.0 * 1024.0).round() as i64);

        // FeedFileNoAttributesTest
        let xml3 = br#"<?xml version="1.0"?><rss xmlns:newznab="http://www.newznab.com/DTD/2010/feeds/attributes/"><channel><item><title>a</title><description>Size: 300</description><enclosure/><newznab:attr/><enclosure url="http://x/a.nzb"/></item></channel></rss>"#;
        let mut p3 = FeedFileParser::parse_xml(xml3).unwrap();
        let items3 = p3.finalize_items();
        assert_eq!(items3.len(), 1);
        assert_eq!(items3[0].url.as_bytes(), b"http://x/a.nzb");
        assert_eq!(items3[0].size, 300);

        // Duplicate/malformed attributes and multiple roots are rejected, and ISO-8859-1 CDATA is decoded to UTF-8
        assert!(FeedFileParser::parse_xml(
            br#"<rss><channel><item><enclosure url="a" url="b"/></item></channel></rss>"#
        )
        .is_none());
        assert!(FeedFileParser::parse_xml(
            br#"<rss><channel><item><title>a</title></item></channel></rss><rss><channel><item><title>b</title></item></channel></rss>"#
        )
        .is_none());
        assert!(FeedFileParser::parse_xml(
            br#"<rss><channel><item><title>a</title></item></channel></rss>trailing"#
        )
        .is_none());
        assert!(FeedFileParser::parse_xml(
            br#"<!DOCTYPE rss [<!ENTITY xxe "secret">]><rss><channel><item><title>a</title></item></channel></rss>"#
        )
        .is_none());
        let cdata_entity_literal = br#"<rss><channel><item><title><![CDATA[Understanding <!ENTITY declarations]]></title></item></channel></rss>"#;
        let mut p_cdata_ent = FeedFileParser::parse_xml(cdata_entity_literal).unwrap();
        assert_eq!(
            p_cdata_ent.finalize_items()[0].title.as_bytes(),
            b"Understanding <!ENTITY declarations"
        );
        let latin1_cdata = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><rss><channel><item><title><![CDATA[Caf\xe9\r\nBeta\rGamma]]></title><enclosure url=\"http://x/?q=&amp;amp;\"/><newznab:attr name=\"tag\" value=\"Group\tName\r\nNext&#9;Tab\"/></item></channel></rss>";
        let mut p4 = FeedFileParser::parse_xml(latin1_cdata).unwrap();
        let items4 = p4.finalize_items();
        assert_eq!(items4[0].title.as_bytes(), "Café\nBeta\nGamma".as_bytes());
        assert_eq!(items4[0].url.as_bytes(), b"http://x/?q=&amp;");
        assert_eq!(items4[0].attr_values[0].as_bytes(), b"Group Name Next\tTab");
        assert_eq!(
            crate::paths::make_valid_filename(
                &normalize_feed_nzb_name(items4[0].filename.as_bytes()),
                false
            ),
            "Café_Beta_Gamma.nzb".as_bytes()
        );
        assert!(is_feed_download_hanging(200, 100, 60, 1, 1));
        assert!(!is_feed_download_hanging(170, 100, 60, 1, 1));
        assert!(!is_feed_download_hanging(200, 100, 0, 1, 1));
        assert!(is_feed_url_and_filter_updated(
            b"https://example.com/rss",
            b"https://example.com/rss",
            b"A: *",
            b"A: *",
            12345
        ));
        assert!(!is_feed_url_and_filter_updated(
            b"https://example.com/rss",
            b"https://example.com/rss",
            b"A: *",
            b"A: *",
            0
        ));
    }
}
