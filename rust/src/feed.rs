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
    let season_num =
        crate::util::util_str_to_i64_checked(season_slice, i32::MIN as i64, i32::MAX as i64)
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
                        if !rest
                            .bytes()
                            .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                        {
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
pub fn should_delete_history_entry(last_seen: i64, border_date: i64, url: &[u8]) -> Option<usize> {
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
// 4. End-to-End FeedCoordinator, FeedInfo, FeedItemInfo, FeedFilter & FeedHistory Engine
// ============================================================================

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const FS_UNDEFINED: i32 = 0;
const FS_RUNNING: i32 = 1;
const FS_FINISHED: i32 = 2;
const FS_FAILED: i32 = 3;

const AD_RUNNING: i32 = 1;
const AD_FINISHED: i32 = 2;
const AD_FAILED: i32 = 3;
const AD_RETRY: i32 = 4;

const MS_IGNORED: i32 = 0;
const MS_ACCEPTED: i32 = 1;
const MS_REJECTED: i32 = 2;

const HS_FETCHED: i32 = 2;
const EA_URL_COMPLETED: i32 = 10;

fn feed_bytes_to_cstr(raw: &[u8]) -> CString {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    unsafe { CString::from_vec_unchecked(raw[..end].to_vec()) }
}

fn feed_cstr_bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        b""
    } else {
        unsafe { CStr::from_ptr(p) }.to_bytes()
    }
}

fn feed_now_sec() -> i64 {
    #[cfg(unix)]
    unsafe {
        libc::time(std::ptr::null_mut()) as i64
    }
    #[cfg(not(unix))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

fn feed_rand_int() -> i32 {
    #[cfg(unix)]
    unsafe {
        libc::rand()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

#[cfg(unix)]
fn feed_bytes_to_path(bytes: &[u8]) -> &std::path::Path {
    use std::os::unix::ffi::OsStrExt;
    std::path::Path::new(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn feed_bytes_to_path(bytes: &[u8]) -> &std::path::Path {
    std::path::Path::new(std::str::from_utf8(bytes).unwrap_or(""))
}

#[derive(Clone)]
struct RsFeedConfig {
    id: i32,
    name: CString,
    url: CString,
    filter: Vec<u8>,
    category: Vec<u8>,
    filter_hash: u32,
    category_source: i32,
    interval: i32,
    priority: i32,
    backlog: bool,
    pause_nzb: bool,
    cert_verif_level: u32,
}

struct RsFeedInfo {
    cfg: RsFeedConfig,
    output_filename: Vec<u8>,
    last_update: i64,
    next_update: i64,
    last_interval: i32,
    status: i32,
    fetch: bool,
    force: bool,
}

struct PreviewFeedSlot {
    cfg: RsFeedConfig,
    status: AtomicI32,
    last_interval: AtomicI32,
    output_filename: Mutex<Vec<u8>>,
}

#[derive(Clone)]
enum ActiveFeedTarget {
    Configured(usize),
    Preview(Arc<PreviewFeedSlot>),
}

struct ActiveFeedDownload {
    #[cfg(unix)]
    dl: Arc<crate::net::NzbgetRsWebDl>,
    #[cfg(not(unix))]
    dl_ptr: usize,
    target: ActiveFeedTarget,
}

#[derive(Clone)]
struct RsFeedAttr {
    name: CString,
    value: Vec<u8>,
}

#[derive(Clone)]
struct RsFeedItem {
    title: Vec<u8>,
    filename: Vec<u8>,
    url: Vec<u8>,
    category: Vec<u8>,
    description: Vec<u8>,
    season: Vec<u8>,
    episode: Vec<u8>,
    add_category: Vec<u8>,
    dupe_key: Vec<u8>,
    dupe_status: Vec<u8>,
    time: i64,
    size: i64,
    imdb_id: i32,
    rage_id: i32,
    tvdb_id: i32,
    tvmaze_id: i32,
    season_num: i32,
    episode_num: i32,
    season_episode_parsed: bool,
    pause_nzb: bool,
    priority: i32,
    status: i32,
    match_status: i32,
    match_rule: i32,
    dupe_score: i32,
    dupe_mode: i32,
    attrs: Vec<RsFeedAttr>,
}

impl RsFeedItem {
    fn from_parsed(p: &ParsedFeedItem) -> Self {
        let season = if p.has_season {
            p.season.as_bytes().to_vec()
        } else {
            Vec::new()
        };
        let season_num = if p.has_season {
            parse_prefixed_int(&season)
        } else {
            0
        };
        let episode = if p.has_episode {
            p.episode.as_bytes().to_vec()
        } else {
            Vec::new()
        };
        let episode_num = if p.has_episode {
            parse_prefixed_int(&episode)
        } else {
            0
        };
        let attrs = p
            .attr_names
            .iter()
            .zip(p.attr_values.iter())
            .map(|(n, v)| RsFeedAttr {
                name: n.clone(),
                value: v.as_bytes().to_vec(),
            })
            .collect();
        Self {
            title: p.title.as_bytes().to_vec(),
            filename: p.filename.as_bytes().to_vec(),
            url: p.url.as_bytes().to_vec(),
            category: p.category.as_bytes().to_vec(),
            description: p.description.as_bytes().to_vec(),
            season,
            episode,
            add_category: Vec::new(),
            dupe_key: Vec::new(),
            dupe_status: Vec::new(),
            time: p.time,
            size: p.size,
            imdb_id: p.imdb_id,
            rage_id: p.rage_id,
            tvdb_id: p.tvdb_id,
            tvmaze_id: p.tvmaze_id,
            season_num,
            episode_num,
            season_episode_parsed: false,
            pause_nzb: false,
            priority: 0,
            status: 0,
            match_status: MS_IGNORED,
            match_rule: 0,
            dupe_score: 0,
            dupe_mode: 0,
            attrs,
        }
    }

    fn set_season(&mut self, s: Vec<u8>) {
        self.season_num = parse_prefixed_int(&s);
        self.season = s;
    }

    fn set_episode(&mut self, e: Vec<u8>) {
        self.episode_num = parse_prefixed_int(&e);
        self.episode = e;
    }

    fn parse_season_episode_lazy(&mut self) {
        self.season_episode_parsed = true;
        if let Some((s, e)) = parse_season_episode(&self.title) {
            self.set_season(s);
            self.set_episode(e);
        }
    }

    fn get_season_num(&mut self) -> i32 {
        if self.season.is_empty() && !self.season_episode_parsed {
            self.parse_season_episode_lazy();
        }
        self.season_num
    }

    fn get_episode_num(&mut self) -> i32 {
        if self.episode.is_empty() && !self.season_episode_parsed {
            self.parse_season_episode_lazy();
        }
        self.episode_num
    }

    fn build_dupe_key(&mut self, rage_id: &[u8], tvdb_id: &[u8], tvmaze_id: &[u8], series: &[u8]) {
        let any_series = has_any_series_id(
            series,
            rage_id,
            tvdb_id,
            tvmaze_id,
            self.rage_id,
            self.tvdb_id,
            self.tvmaze_id,
        );
        let s_num = if any_series { self.get_season_num() } else { 0 };
        let e_num = if any_series && s_num != 0 {
            self.get_episode_num()
        } else {
            0
        };
        self.dupe_key = build_feed_dupe_key(
            rage_id,
            tvdb_id,
            tvmaze_id,
            series,
            self.rage_id,
            self.tvdb_id,
            self.tvmaze_id,
            self.imdb_id,
            s_num,
            e_num,
            &self.season,
            &self.episode,
        );
    }

    fn append_dupe_key(&mut self, extra: &[u8]) {
        if !self.dupe_key.is_empty() && !extra.is_empty() {
            self.dupe_key.push(b'-');
            self.dupe_key.extend_from_slice(extra);
        }
    }

    fn find_attr(&self, target: &[u8]) -> Option<&[u8]> {
        let target_c = feed_bytes_to_cstr(target);
        for a in &self.attrs {
            if unsafe { attr_name_eq_ignore_case(a.name.as_ptr(), target_c.as_ptr()) } {
                return Some(&a.value);
            }
        }
        None
    }
}

struct FilterItemAdapter<'a, 'b> {
    item: &'a mut RsFeedItem,
    calc_dupe: &'b mut dyn FnMut(&[u8], &[u8]) -> i32,
}

impl crate::feedfilter::Item for FilterItemAdapter<'_, '_> {
    fn field(&mut self, field: crate::feedfilter::Field, attr: &[u8]) -> (Option<Vec<u8>>, i64) {
        use crate::feedfilter::Field;
        match field {
            Field::Title => (Some(self.item.title.clone()), 0),
            Field::Filename => (Some(self.item.filename.clone()), 0),
            Field::Category => (Some(self.item.category.clone()), 0),
            Field::Url => (Some(self.item.url.clone()), 0),
            Field::Size => (None, self.item.size),
            Field::Age => (None, feed_now_sec().wrapping_sub(self.item.time)),
            Field::ImdbId => (None, i64::from(self.item.imdb_id)),
            Field::RageId => (None, i64::from(self.item.rage_id)),
            Field::TvdbId => (None, i64::from(self.item.tvdb_id)),
            Field::TvmazeId => (None, i64::from(self.item.tvmaze_id)),
            Field::Description => (Some(self.item.description.clone()), 0),
            Field::Season => (None, i64::from(self.item.get_season_num())),
            Field::Episode => (None, i64::from(self.item.get_episode_num())),
            Field::Priority => (None, i64::from(self.item.priority)),
            Field::DupeKey => (Some(self.item.dupe_key.clone()), 0),
            Field::DupeScore => (None, i64::from(self.item.dupe_score)),
            Field::DupeStatus => {
                if self.item.dupe_status.is_empty() {
                    let mask = (self.calc_dupe)(&self.item.title, &self.item.dupe_key);
                    self.item.dupe_status = format_dupe_status_mask(mask);
                }
                (Some(self.item.dupe_status.clone()), 0)
            }
            Field::Attr => (self.item.find_attr(attr).map(<[u8]>::to_vec), 0),
        }
    }

    fn season_episode(&mut self, episode: bool) -> Option<Vec<u8>> {
        if episode {
            self.item.get_episode_num();
            Some(self.item.episode.clone())
        } else {
            self.item.get_season_num();
            Some(self.item.season.clone())
        }
    }

    fn apply(&mut self, o: &crate::feedfilter::Applied<'_>) {
        if let Some(p) = o.pause {
            self.item.pause_nzb = p;
        }
        if let Some(cat) = o.category {
            self.item.add_category = cat.unwrap_or(b"").to_vec();
        }
        if let Some(prio) = o.priority {
            self.item.priority = prio;
        }
        if let Some(ap) = o.add_priority {
            self.item.priority = self.item.priority.wrapping_add(ap);
        }
        if let Some(ds) = o.dupe_score {
            self.item.dupe_score = ds;
        }
        if let Some(ads) = o.add_dupe_score {
            self.item.dupe_score = self.item.dupe_score.wrapping_add(ads);
        }
        if let Some(ids) = o.build_dupe_key {
            self.item.build_dupe_key(
                ids[0].unwrap_or(b""),
                ids[1].unwrap_or(b""),
                ids[2].unwrap_or(b""),
                ids[3].unwrap_or(b""),
            );
        }
        if let Some(dk) = o.dupe_key {
            self.item.dupe_key = dk.unwrap_or(b"").to_vec();
        }
        if let Some(adk) = o.add_dupe_key {
            self.item.append_dupe_key(adk.unwrap_or(b""));
        }
        if let Some(dm) = o.dupe_mode {
            self.item.dupe_mode = dm as i32;
        }
    }

    fn set_match(&mut self, status: i32, rule: i32) {
        self.item.match_status = match status {
            1 => MS_ACCEPTED,
            2 => MS_REJECTED,
            _ => MS_IGNORED,
        };
        self.item.match_rule = rule;
    }
}

struct RsFeedHistoryEntry {
    url: Vec<u8>,
    status: i32,
    last_seen: i64,
}

struct RsFeedCacheEntry {
    url: Vec<u8>,
    cache_time_sec: i32,
    cache_id: Vec<u8>,
    last_usage: i64,
    items: Arc<Mutex<Vec<RsFeedItem>>>,
}

struct FeedCoordInner {
    feeds: Vec<RsFeedInfo>,
    active: Vec<ActiveFeedDownload>,
    history: Vec<RsFeedHistoryEntry>,
    cache: Vec<RsFeedCacheEntry>,
    save: bool,
}

/// Stateful Rust engine backing `FeedCoordinator`, `FeedInfo`, `FeedItemInfo`, `FeedFilter`, and `FeedHistory`.
pub struct NzbgetRsFeedCoord {
    state: Mutex<FeedCoordInner>,
    wait_cv: Condvar,
    force: AtomicBool,
    daemon_stopped: AtomicBool,
    daemon_running: AtomicBool,
    preview_num: AtomicI32,
    host_env: Mutex<Option<NzbgetRsFeedCoordEnv>>,
    self_weak: Mutex<std::sync::Weak<NzbgetRsFeedCoord>>,
}

impl Default for NzbgetRsFeedCoord {
    fn default() -> Self {
        Self::new()
    }
}

/// Host C++ callback environment for `nzbget_rs_feed_coord_exec`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NzbgetRsFeedCoordEnv {
    pub ctx: *mut c_void,
    pub p1: usize,
    pub a1: c_int,
    pub a2: c_int,
    pub a3: c_int,
    pub a4: c_int,
    pub a5: c_int,
    pub a6: c_int,
    pub u1: u32,
    pub s1: *const c_char,
    pub s2: *const c_char,
    pub s3: *const c_char,
    pub s4: *const c_char,
    pub s5: *const c_char,
    pub s6: *const c_char,
    pub log_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    pub query_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            usize,
            *mut c_int,
            *mut i64,
            *mut *const c_char,
        ) -> usize,
    >,
    pub action_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            usize,
            c_int,
            c_int,
            c_int,
            i64,
            i64,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
        ) -> usize,
    >,
    pub rpc_emit_cb:
        Option<unsafe extern "C" fn(*mut c_void, *const crate::xmlrpc_format::NzbgetRsRpcFeedItem)>,
}

unsafe impl Send for NzbgetRsFeedCoordEnv {}
unsafe impl Sync for NzbgetRsFeedCoordEnv {}

fn feed_emit_log(env: &NzbgetRsFeedCoordEnv, level: i32, parts: &[&[u8]]) {
    if let Some(cb) = env.log_cb {
        let joined: Vec<u8> = parts.concat();
        let cstr = feed_bytes_to_cstr(&joined);
        unsafe { cb(env.ctx, level as c_int, cstr.as_ptr()) };
        return;
    }
    #[cfg(unix)]
    {
        if level == 0 && !crate::log::active_debug_build() {
            return;
        }
        let kind = match level {
            0 => 3,
            1 => 4,
            2 => 0,
            3 => 1,
            _ => 2,
        };
        let joined: Vec<u8> = parts.concat();
        crate::log::log_active_bytes(kind, &joined);
    }
}

struct RuntimeOpts {
    server_mode: bool,
    pause_download: bool,
    url_force: bool,
    url_connections: i32,
    url_timeout: i32,
    feed_history_days: i32,
    flush_queue: bool,
    path_sep: u8,
    temp_dir: Vec<u8>,
    queue_dir: Vec<u8>,
}

fn query_runtime_opts(env: &NzbgetRsFeedCoordEnv) -> RuntimeOpts {
    if let Some(opts) = crate::options::active_options() {
        let sc = opts.scalars();
        let pause_download = crate::ffi::active_workstate().is_some_and(|w| w.pause_download());
        return RuntimeOpts {
            server_mode: opts.server_mode(),
            pause_download,
            url_force: sc.url_force != 0,
            url_connections: sc.url_connections,
            url_timeout: sc.url_timeout,
            feed_history_days: sc.feed_history,
            flush_queue: sc.flush_queue != 0,
            path_sep: crate::paths::PATH_SEPARATOR as u8,
            temp_dir: opts.str_bytes(5).to_vec(),
            queue_dir: opts.str_bytes(6).to_vec(),
        };
    }
    let mut ints = [0 as c_int; 8];
    let mut strs = [std::ptr::null::<c_char>(); 4];
    if let Some(q) = env.query_cb {
        unsafe {
            q(
                env.ctx,
                2,
                0,
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
    }
    RuntimeOpts {
        server_mode: ints[0] != 0,
        pause_download: ints[1] != 0,
        url_force: ints[2] != 0,
        url_connections: ints[3],
        url_timeout: ints[4],
        feed_history_days: ints[5],
        flush_queue: ints[6] != 0,
        path_sep: ints[7] as u8,
        temp_dir: feed_cstr_bytes(strs[0]).to_vec(),
        queue_dir: feed_cstr_bytes(strs[1]).to_vec(),
    }
}

struct DiskBridgeCtx<'a> {
    inner: &'a mut FeedCoordInner,
    env: &'a NzbgetRsFeedCoordEnv,
}

unsafe extern "C" fn feed_disk_log_cb(raw: *mut c_void, level: c_int, msg: *const c_char) {
    let bridge = unsafe { &*raw.cast::<DiskBridgeCtx<'_>>() };
    if let Some(cb) = bridge.env.log_cb {
        unsafe { cb(bridge.env.ctx, level, msg) };
        return;
    }
    #[cfg(unix)]
    {
        if level == 0 && !crate::log::active_debug_build() {
            return;
        }
        let kind = match level {
            0 => 3,
            1 => 4,
            2 => 0,
            3 => 1,
            _ => 2,
        };
        crate::log::log_active_bytes(kind, feed_cstr_bytes(msg));
    }
}

unsafe extern "C" fn feed_disk_aux_cb(
    raw: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    u1: u32,
    s1: *const c_char,
    s2: *const c_char,
    _vol_item: *const c_void,
    sink_ctx: *mut c_void,
    sink_fn: Option<crate::diskstate_format::DiskAuxSinkFn>,
) {
    let bridge = unsafe { &mut *raw.cast::<DiskBridgeCtx<'_>>() };
    match op {
        1 => {
            let Some(sink) = sink_fn else {
                return;
            };
            for fi in &bridge.inner.feeds {
                unsafe {
                    sink(
                        sink_ctx,
                        0,
                        fi.last_update as c_int,
                        0,
                        fi.cfg.filter_hash,
                        fi.cfg.url.as_ptr(),
                        std::ptr::null(),
                        0,
                        std::ptr::null(),
                        0,
                    );
                }
            }
            for h in &bridge.inner.history {
                let c_url = feed_bytes_to_cstr(&h.url);
                unsafe {
                    sink(
                        sink_ctx,
                        1,
                        h.status as c_int,
                        h.last_seen as c_int,
                        0,
                        c_url.as_ptr(),
                        std::ptr::null(),
                        0,
                        std::ptr::null(),
                        0,
                    );
                }
            }
        }
        2 => {
            let url = feed_cstr_bytes(s1);
            let filter = feed_cstr_bytes(s2);
            for fi in &mut bridge.inner.feeds {
                if fi.cfg.url.as_bytes() == url
                    && (a2 == 1
                        || (a2 == 2 && fi.cfg.filter == filter)
                        || (a2 >= 3 && fi.cfg.filter_hash == u1))
                {
                    fi.last_update = i64::from(a1);
                }
            }
        }
        3 => {
            let url = feed_cstr_bytes(s1).to_vec();
            bridge.inner.history.push(RsFeedHistoryEntry {
                url,
                status: a1,
                last_seen: i64::from(a2),
            });
        }
        _ => {}
    }
}

fn run_diskstate_feeds(inner: &mut FeedCoordInner, env: &NzbgetRsFeedCoordEnv, save: bool) {
    let opts = query_runtime_opts(env);
    let qdir_c = feed_bytes_to_cstr(&opts.queue_dir);
    let mut bridge = DiskBridgeCtx { inner, env };
    let mut ds_env: crate::diskstate_format::NzbgetRsDiskStateEnv = unsafe { std::mem::zeroed() };
    ds_env.ctx = (&mut bridge as *mut DiskBridgeCtx<'_>).cast();
    ds_env.queue_dir = qdir_c.as_ptr();
    ds_env.sep = c_int::from(opts.path_sep);
    ds_env.flush_queue = c_int::from(opts.flush_queue);
    ds_env.log_cb = Some(feed_disk_log_cb);
    ds_env.aux_cb = Some(feed_disk_aux_cb);
    unsafe {
        crate::diskstate_format::diskstate_exec(if save { 9 } else { 10 }, &ds_env);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_feed_config(
    id: i32,
    name: &[u8],
    url: &[u8],
    backlog: bool,
    interval: i32,
    filter: &[u8],
    pause_nzb: bool,
    category: &[u8],
    category_source: i32,
    priority: i32,
    cert_verif_level: u32,
) -> RsFeedConfig {
    let (def_name, filter_hash) = feed_info_init(id, name, filter);
    let final_name = def_name.unwrap_or_else(|| name.to_vec());
    RsFeedConfig {
        id,
        name: feed_bytes_to_cstr(&final_name),
        url: feed_bytes_to_cstr(url),
        filter: filter.to_vec(),
        category: category.to_vec(),
        filter_hash,
        category_source,
        interval,
        priority,
        backlog,
        pause_nzb,
        cert_verif_level,
    }
}

impl NzbgetRsFeedCoord {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FeedCoordInner {
                feeds: Vec::new(),
                active: Vec::new(),
                history: Vec::new(),
                cache: Vec::new(),
                save: false,
            }),
            wait_cv: Condvar::new(),
            force: AtomicBool::new(false),
            daemon_stopped: AtomicBool::new(false),
            daemon_running: AtomicBool::new(false),
            preview_num: AtomicI32::new(0),
            host_env: Mutex::new(None),
            self_weak: Mutex::new(std::sync::Weak::new()),
        }
    }

    fn effective_env(&self, caller: Option<&NzbgetRsFeedCoordEnv>) -> NzbgetRsFeedCoordEnv {
        let mut base = self
            .host_env
            .lock()
            .ok()
            .and_then(|g| *g)
            .unwrap_or_else(|| unsafe { std::mem::zeroed() });
        if let Some(c) = caller {
            base.p1 = c.p1;
            base.a1 = c.a1;
            base.a2 = c.a2;
            base.a3 = c.a3;
            base.a4 = c.a4;
            base.a5 = c.a5;
            base.a6 = c.a6;
            base.u1 = c.u1;
            base.s1 = c.s1;
            base.s2 = c.s2;
            base.s3 = c.s3;
            base.s4 = c.s4;
            base.s5 = c.s5;
            base.s6 = c.s6;
            if c.log_cb.is_some() {
                base.log_cb = c.log_cb;
            }
            if c.query_cb.is_some() {
                base.query_cb = c.query_cb;
                if !c.ctx.is_null() {
                    base.ctx = c.ctx;
                }
            }
            if c.action_cb.is_some() {
                base.action_cb = c.action_cb;
                if !c.ctx.is_null() {
                    base.ctx = c.ctx;
                }
            }
            if c.rpc_emit_cb.is_some() {
                base.rpc_emit_cb = c.rpc_emit_cb;
            }
        }
        base
    }

    fn is_stopped(&self, env: &NzbgetRsFeedCoordEnv) -> bool {
        let Some(q) = env.query_cb else {
            return self.daemon_stopped.load(Ordering::SeqCst);
        };
        unsafe {
            q(
                env.ctx,
                1,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_feed_direct(
        &self,
        id: i32,
        name: &[u8],
        url: &[u8],
        backlog: bool,
        interval: i32,
        filter: &[u8],
        pause_nzb: bool,
        category: &[u8],
        category_source: i32,
        priority: i32,
        cert_verif_level: u32,
    ) {
        let cfg = build_feed_config(
            id,
            name,
            url,
            backlog,
            interval,
            filter,
            pause_nzb,
            category,
            category_source,
            priority,
            cert_verif_level,
        );
        if let Ok(mut st) = self.state.lock() {
            st.feeds.push(RsFeedInfo {
                cfg,
                output_filename: Vec::new(),
                last_update: 0,
                next_update: 0,
                last_interval: 0,
                status: FS_UNDEFINED,
                fetch: false,
                force: false,
            });
        }
    }

    fn spawn_downloader(
        &self,
        st: &mut FeedCoordInner,
        target: ActiveFeedTarget,
        cfg: &RsFeedConfig,
        preview: bool,
        force: bool,
        opts: &RuntimeOpts,
        env: &NzbgetRsFeedCoordEnv,
    ) -> usize {
        feed_emit_log(
            env,
            0,
            &[b"Starting new FeedDownloader for ", cfg.name.as_bytes()],
        );
        let pnum = self.preview_num.fetch_add(1, Ordering::Relaxed) + 1;
        let out_fn = format_temp_filename(
            &opts.temp_dir,
            opts.path_sep,
            cfg.id,
            preview,
            feed_now_sec() as i32,
            feed_rand_int(),
            pnum,
        );
        #[cfg(unix)]
        {
            let cert_level = if crate::ffi::build_tls_disabled() {
                2
            } else {
                cfg.cert_verif_level as c_int
            };
            let weak = self.self_weak.lock().map(|g| g.clone()).unwrap_or_default();
            let dl = Arc::new(crate::net::NzbgetRsWebDl::new_configured(
                cfg.url.as_bytes(),
                cfg.name.as_bytes(),
                &out_fn,
                force || opts.url_force,
                true,
                cert_level,
                Some(Box::new(move |dl_ref, st_enum| {
                    if let Some(fc) = weak.upgrade() {
                        fc.on_webdl_status(dl_ref, st_enum as i32);
                    }
                })),
                None,
            ));
            let dl_key = Arc::as_ptr(&dl) as usize;
            match &target {
                ActiveFeedTarget::Configured(idx) => {
                    if let Some(fi) = st.feeds.get_mut(*idx) {
                        fi.status = FS_RUNNING;
                        fi.force = force;
                        fi.fetch = false;
                    }
                }
                ActiveFeedTarget::Preview(slot) => {
                    slot.status.store(FS_RUNNING, Ordering::Release);
                }
            }
            st.active.push(ActiveFeedDownload {
                dl: Arc::clone(&dl),
                target,
            });
            crate::net::NzbgetRsWebDl::spawn(dl);
            return dl_key;
        }
        #[cfg(not(unix))]
        {
            let _ = (st, target, out_fn);
            0
        }
    }

    fn check_save_feeds(&self, env: &NzbgetRsFeedCoordEnv) {
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        if st.save {
            feed_emit_log(env, 0, &[b"CheckSaveFeeds: save"]);
            let opts = query_runtime_opts(env);
            if opts.server_mode {
                run_diskstate_feeds(&mut st, env, true);
            }
            st.save = false;
        }
    }

    fn reset_hanging_downloads(&self, env: &NzbgetRsFeedCoordEnv) {
        let opts = query_runtime_opts(env);
        if opts.url_timeout == 0 {
            return;
        }
        let Ok(st) = self.state.lock() else {
            return;
        };
        let tm = feed_now_sec();
        for active in &st.active {
            #[cfg(unix)]
            {
                let st_code = active.dl.status_code();
                let last_upd = active.dl.last_update_time();
                if is_feed_download_hanging(tm, last_upd, opts.url_timeout, st_code, AD_RUNNING) {
                    let info_name = active.dl.info_name_bytes();
                    feed_emit_log(env, 4, &[b"Cancelling hanging feed download ", &info_name]);
                    active.dl.stop();
                }
            }
            #[cfg(not(unix))]
            {
                let _ = (tm, active.dl_ptr);
            }
        }
    }

    fn cleanup_history(&self, env: &NzbgetRsFeedCoordEnv) {
        feed_emit_log(env, 0, &[b"CleanupHistory"]);
        let opts = query_runtime_opts(env);
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        let updates: Vec<i64> = st.feeds.iter().map(|f| f.last_update).collect();
        let border = calc_history_border_date(feed_now_sec(), &updates, opts.feed_history_days);
        let mut changed = false;
        st.history.retain(|entry| {
            if let Some(prefix_len) =
                should_delete_history_entry(entry.last_seen, border, &entry.url)
            {
                feed_emit_log(
                    env,
                    1,
                    &[
                        b"Deleting ",
                        &entry.url[..prefix_len],
                        b" from feed history",
                    ],
                );
                changed = true;
                false
            } else {
                true
            }
        });
        if changed {
            st.save = true;
        }
    }

    fn cleanup_cache(&self, env: &NzbgetRsFeedCoordEnv) {
        feed_emit_log(env, 0, &[b"CleanupCache"]);
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        let cur_time = feed_now_sec();
        st.cache.retain(|item| {
            if is_feed_cache_expired(item.last_usage, item.cache_time_sec, cur_time) {
                feed_emit_log(env, 0, &[b"Deleting ", &item.url, b" from feed cache"]);
                false
            } else {
                true
            }
        });
    }

    fn parse_feed_items(
        output_filename: &[u8],
        info_name: &[u8],
        env: &NzbgetRsFeedCoordEnv,
    ) -> Option<Vec<RsFeedItem>> {
        feed_emit_log(env, 0, &[b"Creating FeedFile"]);
        if output_filename.is_empty() {
            feed_emit_log(env, 4, &[b"Failed to parse rss feed ", info_name]);
            feed_emit_log(
                env,
                4,
                &[
                    b"Feed file ",
                    output_filename,
                    b" kept for troubleshooting (will be deleted on next successful feed fetch)",
                ],
            );
            return None;
        }
        let Some(mut parser) = FeedFileParser::parse_file(feed_bytes_to_path(output_filename))
        else {
            feed_emit_log(env, 4, &[b"Failed to parse rss feed ", info_name]);
            feed_emit_log(
                env,
                4,
                &[
                    b"Feed file ",
                    output_filename,
                    b" kept for troubleshooting (will be deleted on next successful feed fetch)",
                ],
            );
            return None;
        };
        let parsed = parser.finalize_items();
        let out: Vec<RsFeedItem> = parsed.iter().map(RsFeedItem::from_parsed).collect();
        let fn_c = feed_bytes_to_cstr(output_filename);
        crate::fsops::delete_file(&fn_c);
        Some(out)
    }

    fn filter_feed_items(cfg: &RsFeedConfig, items: &mut [RsFeedItem], env: &NzbgetRsFeedCoordEnv) {
        feed_emit_log(env, 0, &[b"Filtering feed ", cfg.name.as_bytes()]);
        let mut filter =
            (!cfg.filter.is_empty()).then(|| crate::feedfilter::FeedFilter::new(&cfg.filter));
        let mut calc_dupe = |title: &[u8], dupe_key: &[u8]| -> i32 {
            if let Some(act) = env.action_cb {
                let t_c = feed_bytes_to_cstr(title);
                let k_c = feed_bytes_to_cstr(dupe_key);
                return unsafe {
                    act(
                        env.ctx,
                        2,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        t_c.as_ptr(),
                        k_c.as_ptr(),
                        std::ptr::null(),
                        std::ptr::null(),
                    ) as i32
                };
            }
            #[cfg(unix)]
            if let Some(qh) = crate::ffi::queue_host() {
                return qh.with_locked_queue(|dq_ptr| {
                    crate::dupecoord::get_dupe_status_host(&qh, dq_ptr, title, dupe_key)
                });
            }
            0
        };

        for item in items.iter_mut() {
            item.match_status = MS_ACCEPTED;
            item.match_rule = 0;
            item.pause_nzb = cfg.pause_nzb;
            item.priority = cfg.priority;
            item.add_category =
                resolve_category(&cfg.category, cfg.category_source, &item.category).to_vec();
            item.dupe_score = 0;
            item.dupe_mode = 0;
            item.build_dupe_key(b"", b"", b"", b"");
            if let Some(ref mut ff) = filter {
                let mut adapter = FilterItemAdapter {
                    item,
                    calc_dupe: &mut calc_dupe,
                };
                ff.matches(&mut adapter);
            }
        }
    }

    fn run_loop(&self, env: &NzbgetRsFeedCoordEnv) {
        feed_emit_log(env, 0, &[b"Entering FeedCoordinator-loop"]);

        while !crate::ffi::is_queue_loaded() {
            if let Some(q) = env.query_cb {
                if unsafe {
                    q(
                        env.ctx,
                        0,
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                } != 0
                {
                    break;
                }
            } else if self.daemon_stopped.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let init_opts = query_runtime_opts(env);
        if init_opts.server_mode {
            if let Ok(mut st) = self.state.lock() {
                run_diskstate_feeds(&mut st, env, false);
            }
        }

        let mut last_cleanup: i64 = 0;
        while !self.is_stopped(env) {
            let opts = query_runtime_opts(env);
            if !opts.pause_download || self.force.load(Ordering::SeqCst) || opts.url_force {
                if let Ok(mut st) = self.state.lock() {
                    let current = feed_now_sec();
                    if (st.active.len() as i32) < opts.url_connections {
                        self.force.store(false, Ordering::SeqCst);
                        let mut to_start = Vec::new();
                        for (idx, fi) in st.feeds.iter().enumerate() {
                            let (do_start, set_force) = should_start_feed(
                                fi.cfg.interval,
                                fi.next_update,
                                current,
                                fi.fetch,
                                fi.status,
                                FS_RUNNING,
                            );
                            if do_start {
                                to_start.push((idx, fi.fetch, fi.cfg.clone()));
                            } else if set_force {
                                self.force.store(true, Ordering::SeqCst);
                            }
                        }
                        for (idx, force_flag, cfg_clone) in to_start {
                            let _ = self.spawn_downloader(
                                &mut st,
                                ActiveFeedTarget::Configured(idx),
                                &cfg_clone,
                                false,
                                force_flag,
                                &opts,
                                env,
                            );
                        }
                    }
                }
            }

            self.check_save_feeds(env);
            self.reset_hanging_downloads(env);

            let now = feed_now_sec();
            if (now - last_cleanup).abs() >= 60 {
                self.cleanup_history(env);
                self.cleanup_cache(env);
                self.check_save_feeds(env);
                last_cleanup = feed_now_sec();
            }

            if let Ok(lk) = self.state.lock() {
                if self.force.load(Ordering::SeqCst) {
                    let _ =
                        self.wait_cv
                            .wait_timeout_while(lk, Duration::from_millis(1000), |_| {
                                !self.is_stopped(env)
                            });
                } else {
                    let wait_ms = if opts.url_force || lk.feeds.is_empty() {
                        60_000
                    } else {
                        5_000
                    };
                    let _ =
                        self.wait_cv
                            .wait_timeout_while(lk, Duration::from_millis(wait_ms), |_| {
                                !self.force.load(Ordering::SeqCst) && !self.is_stopped(env)
                            });
                }
            }
        }

        feed_emit_log(
            env,
            0,
            &[b"FeedCoordinator: waiting for Downloads to complete"],
        );
        loop {
            let completed = self
                .state
                .lock()
                .map(|st| st.active.is_empty())
                .unwrap_or(true);
            self.check_save_feeds(env);
            if completed {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            self.reset_hanging_downloads(env);
        }
        feed_emit_log(env, 0, &[b"FeedCoordinator: Downloads are completed"]);
        feed_emit_log(env, 0, &[b"Exiting FeedCoordinator-loop"]);
    }

    fn stop_downloads(&self, env: &NzbgetRsFeedCoordEnv) {
        feed_emit_log(env, 0, &[b"Stopping UrlDownloads"]);
        if let Ok(st) = self.state.lock() {
            for active in &st.active {
                #[cfg(unix)]
                active.dl.stop();
                #[cfg(not(unix))]
                let _ = active.dl_ptr;
            }
            feed_emit_log(env, 0, &[b"UrlDownloads are notified"]);
            self.wait_cv.notify_all();
        }
    }

    #[cfg(unix)]
    fn on_webdl_status(&self, dl: &crate::net::NzbgetRsWebDl, st_code: i32) {
        let env = self.effective_env(None);
        feed_emit_log(&env, 0, &[b"Notification from FeedDownloader received"]);
        if st_code != AD_FINISHED && st_code != AD_FAILED && st_code != AD_RETRY {
            return;
        }

        feed_emit_log(&env, 0, &[b"Feed downloaded"]);
        let status_ok = st_code == AD_FINISHED;
        let out_fn = dl.output_filename_bytes();
        let dl_key = dl as *const crate::net::NzbgetRsWebDl;

        let removed_target = {
            let Ok(mut st) = self.state.lock() else {
                return;
            };
            let Some(pos) = st.active.iter().position(|a| Arc::as_ptr(&a.dl) == dl_key) else {
                return;
            };
            let target = st.active.remove(pos).target;
            match &target {
                ActiveFeedTarget::Configured(idx) => {
                    if let Some(fi) = st.feeds.get_mut(*idx) {
                        if status_ok {
                            fi.output_filename = out_fn.clone();
                        }
                        let (interval, new_last) =
                            calc_feed_next_interval(status_ok, fi.cfg.interval, fi.last_interval);
                        fi.last_interval = new_last;
                        fi.next_update = feed_now_sec().wrapping_add(i64::from(interval));
                        let name_bytes = fi.cfg.name.as_bytes().to_vec();
                        feed_emit_log(
                            &env,
                            1,
                            &[
                                b"Scheduling update for feed ",
                                &name_bytes,
                                format!(" in {} minute(s)", interval / 60).as_bytes(),
                            ],
                        );
                    }
                }
                ActiveFeedTarget::Preview(slot) => {
                    if status_ok {
                        if let Ok(mut g) = slot.output_filename.lock() {
                            *g = out_fn.clone();
                        }
                    }
                    let prev_last = slot.last_interval.load(Ordering::Relaxed);
                    let (interval, new_last) =
                        calc_feed_next_interval(status_ok, slot.cfg.interval, prev_last);
                    slot.last_interval.store(new_last, Ordering::Relaxed);
                    feed_emit_log(
                        &env,
                        1,
                        &[
                            b"Scheduling update for feed ",
                            slot.cfg.name.as_bytes(),
                            format!(" in {} minute(s)", interval / 60).as_bytes(),
                        ],
                    );
                }
            }
            target
        };

        match removed_target {
            ActiveFeedTarget::Preview(slot) => {
                slot.status.store(
                    if status_ok { FS_FINISHED } else { FS_FAILED },
                    Ordering::Release,
                );
            }
            ActiveFeedTarget::Configured(idx) => {
                if !status_ok {
                    if let Ok(mut st) = self.state.lock() {
                        if let Some(fi) = st.feeds.get_mut(idx) {
                            fi.status = FS_FAILED;
                        }
                    }
                    return;
                }

                let (cfg, feed_out_fn) = {
                    let Ok(st) = self.state.lock() else {
                        return;
                    };
                    let Some(fi) = st.feeds.get(idx) else {
                        return;
                    };
                    (fi.cfg.clone(), fi.output_filename.clone())
                };

                let mut parsed_items =
                    Self::parse_feed_items(&feed_out_fn, cfg.name.as_bytes(), &env);
                if let Some(ref mut items) = parsed_items {
                    feed_emit_log(&env, 0, &[b"Process feed ", cfg.name.as_bytes()]);
                    Self::filter_feed_items(&cfg, items, &env);
                }

                let mut added_nzbs = Vec::new();
                if let Ok(mut st) = self.state.lock() {
                    if let Some(items) = parsed_items {
                        let first_fetch = st
                            .feeds
                            .get(idx)
                            .map(|f| f.last_update == 0)
                            .unwrap_or(true);
                        let feed_in = FeedCreationInput {
                            id: cfg.id,
                            category: &cfg.category,
                            category_source: cfg.category_source,
                        };
                        let now_ts = feed_now_sec();
                        for item in &items {
                            if item.match_status != MS_ACCEPTED {
                                continue;
                            }
                            let hist_pos = st.history.iter().position(|h| h.url == item.url);
                            let (hist_st, should_add) =
                                classify_item_history(first_fetch, cfg.backlog, hist_pos.is_some());
                            if should_add {
                                feed_emit_log(
                                    &env,
                                    0,
                                    &[b"Download ", &item.url, b" from ", cfg.name.as_bytes()],
                                );
                                let item_in = FeedItemCreationInput {
                                    url: &item.url,
                                    filename: &item.filename,
                                    category: &item.category,
                                    add_category: &item.add_category,
                                    size: item.size,
                                    time: item.time,
                                    priority: item.priority,
                                    pause_nzb: item.pause_nzb,
                                    dupe_key: &item.dupe_key,
                                    dupe_score: item.dupe_score,
                                    dupe_mode: item.dupe_mode,
                                };
                                added_nzbs.push(create_nzb_info(&feed_in, &item_in));
                            }
                            if let Some(hp) = hist_pos {
                                st.history[hp].last_seen = now_ts;
                            } else {
                                st.history.push(RsFeedHistoryEntry {
                                    url: item.url.clone(),
                                    status: hist_st,
                                    last_seen: now_ts,
                                });
                            }
                        }

                        if !added_nzbs.is_empty() {
                            feed_emit_log(
                                &env,
                                2,
                                &[
                                    cfg.name.as_bytes(),
                                    format!(" has {} new item(s)", added_nzbs.len()).as_bytes(),
                                ],
                            );
                        } else {
                            feed_emit_log(&env, 1, &[cfg.name.as_bytes(), b" has no new items"]);
                        }
                    }

                    if let Some(fi) = st.feeds.get_mut(idx) {
                        fi.last_update = feed_now_sec();
                        fi.force = false;
                    }
                    st.save = true;
                }

                if let Some(act) = env.action_cb {
                    for created in &added_nzbs {
                        let url_c = feed_bytes_to_cstr(&created.url);
                        let fn_c = feed_bytes_to_cstr(&created.filename);
                        let cat_c = feed_bytes_to_cstr(&created.category);
                        let dk_c = feed_bytes_to_cstr(&created.dupe_key);
                        let packed_flags = (c_int::from(created.add_url_paused) & 1)
                            | ((created.dupe_mode & 0xff) << 1);
                        unsafe {
                            act(
                                env.ctx,
                                4,
                                created.feed_id as usize,
                                packed_flags,
                                created.priority,
                                created.dupe_score,
                                created.size,
                                created.min_time,
                                url_c.as_ptr(),
                                fn_c.as_ptr(),
                                cat_c.as_ptr(),
                                dk_c.as_ptr(),
                            );
                        }
                    }
                } else {
                    #[cfg(unix)]
                    if let Some(qh) = crate::ffi::queue_host() {
                        for created in &added_nzbs {
                            qh.url_coord_add_feed_item(
                                created.feed_id,
                                &created.url,
                                &created.filename,
                                &created.category,
                                created.priority,
                                created.add_url_paused,
                                &created.dupe_key,
                                created.dupe_score,
                                created.dupe_mode,
                                created.size,
                                created.min_time,
                            );
                        }
                    }
                }

                if let Ok(mut st) = self.state.lock() {
                    if let Some(fi) = st.feeds.get_mut(idx) {
                        fi.status = FS_FINISHED;
                    }
                }
            }
        }
    }

    fn on_queue_update_bytes(&self, env: &NzbgetRsFeedCoordEnv, action: i32, url_bytes: &[u8]) {
        feed_emit_log(env, 0, &[b"Notification from URL-Coordinator received"]);
        if action == EA_URL_COMPLETED {
            let url = url_bytes.to_vec();
            if let Ok(mut st) = self.state.lock() {
                if let Some(h) = st.history.iter_mut().find(|h| h.url == url) {
                    h.status = HS_FETCHED;
                } else {
                    st.history.push(RsFeedHistoryEntry {
                        url,
                        status: HS_FETCHED,
                        last_seen: feed_now_sec(),
                    });
                }
                st.save = true;
            }
        }
    }

    fn on_work_state_update(&self) {
        if let Ok(_st) = self.state.lock() {
            self.force.store(true, Ordering::SeqCst);
            self.wait_cv.notify_all();
        }
    }

    pub(crate) fn fetch_feed_active(&self, target_id: i32) {
        let env = self.effective_env(None);
        self.fetch_feed(&env, target_id);
    }

    fn fetch_feed(&self, env: &NzbgetRsFeedCoordEnv, target_id: i32) {
        feed_emit_log(env, 0, &[b"FetchFeeds"]);
        if let Ok(mut st) = self.state.lock() {
            for fi in &mut st.feeds {
                if fi.cfg.id == target_id || target_id == 0 {
                    fi.fetch = true;
                    self.force.store(true, Ordering::SeqCst);
                }
            }
            self.wait_cv.notify_all();
        }
    }

    pub(crate) fn fetch_feed_list_bytes(&self, feed_list: &[u8]) {
        let env = self.effective_env(None);
        self.fetch_feed_list_inner(&env, feed_list);
    }

    fn fetch_feed_list_inner(&self, env: &NzbgetRsFeedCoordEnv, feed_list: &[u8]) {
        for tok in crate::util::tokens(feed_list, b",;") {
            let tok_c = feed_bytes_to_cstr(tok);
            let parsed_id = c_atoi_bytes(tok);
            let matched_fetch_id = self.state.lock().ok().and_then(|st| {
                for fi in &st.feeds {
                    let (matched, fetch_id) = unsafe {
                        crate::scheduler::scheduler_matches_feed_ref(
                            parsed_id,
                            fi.cfg.id,
                            fi.cfg.name.as_ptr(),
                            tok_c.as_ptr(),
                        )
                    };
                    if matched {
                        return Some(fetch_id);
                    }
                }
                None
            });
            if let Some(fid) = matched_fetch_id {
                self.fetch_feed(env, fid);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_view_or_preview_active(
        &self,
        flags: c_int,
        id: c_int,
        category_source: c_int,
        priority: c_int,
        interval: c_int,
        cache_time_sec: c_int,
        name: *const c_char,
        url: *const c_char,
        filter: *const c_char,
        category: *const c_char,
        extensions: *const c_char,
        cache_id: *const c_char,
        emit_ctx: *mut c_void,
        emit_cb: Option<
            unsafe extern "C" fn(*mut c_void, *const crate::xmlrpc_format::NzbgetRsRpcFeedItem),
        >,
    ) -> c_int {
        let mut env = self.effective_env(None);
        env.a1 = flags;
        env.a2 = id;
        env.a3 = category_source;
        env.a4 = priority;
        env.a5 = interval;
        env.a6 = cache_time_sec;
        env.s1 = name;
        env.s2 = url;
        env.s3 = filter;
        env.s4 = category;
        env.s5 = extensions;
        env.s6 = cache_id;
        env.rpc_emit_cb = emit_cb;
        self.view_or_preview(&env, emit_ctx)
    }

    fn view_or_preview(&self, env: &NzbgetRsFeedCoordEnv, emit_ctx: *mut c_void) -> c_int {
        let flags = env.a1;
        let is_preview = (flags & 1) != 0;
        let incl_non_match = (flags & 8) != 0;

        let (cfg, cache_time_sec, cache_id) = if !is_preview {
            let id = env.a2;
            let Ok(st) = self.state.lock() else {
                return 0;
            };
            if id < 1 || (id as usize) > st.feeds.len() {
                return 0;
            }
            let existing = st.feeds[(id - 1) as usize].cfg.clone();
            (existing, 0, Vec::new())
        } else {
            let id = env.a2;
            let cert_level = self
                .state
                .lock()
                .ok()
                .and_then(|st| {
                    if id >= 1 && (id as usize) <= st.feeds.len() {
                        Some(st.feeds[(id - 1) as usize].cfg.cert_verif_level)
                    } else {
                        None
                    }
                })
                .unwrap_or(2);
            let cfg = build_feed_config(
                id,
                feed_cstr_bytes(env.s1),
                feed_cstr_bytes(env.s2),
                (flags & 2) != 0,
                env.a5,
                feed_cstr_bytes(env.s3),
                (flags & 4) != 0,
                feed_cstr_bytes(env.s4),
                env.a3,
                env.a4,
                cert_level,
            );
            (cfg, env.a6, feed_cstr_bytes(env.s6).to_vec())
        };

        feed_emit_log(env, 0, &[b"Preview feed ", cfg.name.as_bytes()]);

        let mut cached_arc: Option<Arc<Mutex<Vec<RsFeedItem>>>> = None;
        if cache_time_sec > 0 && !cache_id.is_empty() {
            if let Ok(mut st) = self.state.lock() {
                if let Some(entry) = st.cache.iter_mut().find(|c| c.cache_id == cache_id) {
                    entry.last_usage = feed_now_sec();
                    cached_arc = Some(Arc::clone(&entry.items));
                }
            }
        }
        let has_cache = cached_arc.is_some();

        let items_arc = if let Some(arc) = cached_arc {
            arc
        } else {
            let opts = query_runtime_opts(env);
            let slot = Arc::new(PreviewFeedSlot {
                cfg: cfg.clone(),
                status: AtomicI32::new(FS_UNDEFINED),
                last_interval: AtomicI32::new(0),
                output_filename: Mutex::new(Vec::new()),
            });

            let first_fetch = {
                let Ok(mut st) = self.state.lock() else {
                    return 0;
                };
                let mut ff = true;
                for fi2 in &st.feeds {
                    if is_feed_url_and_filter_updated(
                        fi2.cfg.url.as_bytes(),
                        cfg.url.as_bytes(),
                        &fi2.cfg.filter,
                        &cfg.filter,
                        fi2.last_update,
                    ) {
                        ff = false;
                        break;
                    }
                }
                let _ = self.spawn_downloader(
                    &mut st,
                    ActiveFeedTarget::Preview(Arc::clone(&slot)),
                    &cfg,
                    true,
                    true,
                    &opts,
                    env,
                );
                self.force.store(true, Ordering::SeqCst);
                self.wait_cv.notify_all();
                ff
            };

            while slot.status.load(Ordering::Acquire) == FS_RUNNING {
                std::thread::sleep(Duration::from_millis(100));
            }

            if slot.status.load(Ordering::Acquire) != FS_FINISHED {
                return 0;
            }

            let out_fn = slot
                .output_filename
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();
            let Some(mut parsed_items) = Self::parse_feed_items(&out_fn, cfg.name.as_bytes(), env)
            else {
                return 0;
            };

            if let Ok(st) = self.state.lock() {
                for item in &mut parsed_items {
                    let hist = st.history.iter().find(|h| h.url == item.url);
                    item.status = resolve_preview_item_status(
                        hist.is_some(),
                        hist.map_or(0, |h| h.status),
                        first_fetch,
                        cfg.backlog,
                    );
                }
            }

            Arc::new(Mutex::new(parsed_items))
        };

        if let Ok(mut items_guard) = items_arc.lock() {
            Self::filter_feed_items(&cfg, &mut items_guard, env);
            if let Some(emit_cb) = env.rpc_emit_cb {
                for fi in items_guard.iter() {
                    if incl_non_match || fi.match_status == MS_ACCEPTED {
                        let title_c = feed_bytes_to_cstr(&fi.title);
                        let fn_c = feed_bytes_to_cstr(&fi.filename);
                        let url_c = feed_bytes_to_cstr(&fi.url);
                        let cat_c = feed_bytes_to_cstr(&fi.category);
                        let add_cat_c = feed_bytes_to_cstr(&fi.add_category);
                        let dk_c = feed_bytes_to_cstr(&fi.dupe_key);
                        let rpc_item = crate::xmlrpc_format::NzbgetRsRpcFeedItem {
                            title: title_c.as_ptr(),
                            filename: fn_c.as_ptr(),
                            url: url_c.as_ptr(),
                            size: fi.size,
                            size_mb: (fi.size / 1024 / 1024) as c_int,
                            category: cat_c.as_ptr(),
                            add_category: add_cat_c.as_ptr(),
                            pause_nzb: c_int::from(fi.pause_nzb),
                            priority: fi.priority,
                            time: fi.time as c_int,
                            match_status: fi.match_status,
                            match_rule: fi.match_rule,
                            dupe_key: dk_c.as_ptr(),
                            dupe_score: fi.dupe_score,
                            dupe_mode: fi.dupe_mode,
                            status: fi.status,
                        };
                        unsafe { emit_cb(emit_ctx, &rpc_item) };
                    }
                }
            }
        }

        if !has_cache && cache_time_sec > 0 && !cache_id.is_empty() {
            if let Ok(mut st) = self.state.lock() {
                st.cache.push(RsFeedCacheEntry {
                    url: cfg.url.as_bytes().to_vec(),
                    cache_time_sec,
                    cache_id,
                    last_usage: feed_now_sec(),
                    items: items_arc,
                });
            }
        }

        1
    }

    pub(crate) fn collect_health_feeds(&self) -> Vec<(i32, Vec<u8>, Vec<u8>, i32, u32)> {
        self.state
            .lock()
            .map(|st| {
                st.feeds
                    .iter()
                    .map(|fi| {
                        (
                            fi.cfg.id,
                            fi.cfg.name.to_bytes().to_vec(),
                            fi.cfg.url.to_bytes().to_vec(),
                            fi.cfg.interval,
                            fi.cfg.cert_verif_level,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn log_debug_info(&self, env: &NzbgetRsFeedCoordEnv) {
        feed_emit_log(env, 2, &[b"   ---------- FeedCoordinator"]);
        if let Ok(st) = self.state.lock() {
            feed_emit_log(
                env,
                2,
                &[format!("    Active Downloads: {}", st.active.len()).as_bytes()],
            );
            for active in &st.active {
                #[cfg(unix)]
                active.dl.log_debug_info();
                #[cfg(not(unix))]
                let _ = active.dl_ptr;
            }
        }
    }

    fn destroy_remaining(&self, env: &NzbgetRsFeedCoordEnv) {
        feed_emit_log(env, 0, &[b"Destroying FeedCoordinator"]);
        if let Ok(mut st) = self.state.lock() {
            st.active.clear();
        }
    }
}

static ACTIVE_FEED_COORD: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsFeedCoord>>> =
    std::sync::RwLock::new(None);

pub(crate) fn active_feed_coord() -> Option<Arc<NzbgetRsFeedCoord>> {
    ACTIVE_FEED_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

impl NzbgetRsFeedCoord {
    pub(crate) fn has_active_downloads(&self) -> bool {
        self.state
            .lock()
            .map(|st| !st.active.is_empty())
            .unwrap_or(false)
    }
}

static DAEMON_FEED_COORD: std::sync::RwLock<Option<Arc<NzbgetRsFeedCoord>>> =
    std::sync::RwLock::new(None);
static DAEMON_FEED_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

#[cfg(unix)]
pub(crate) fn create_daemon_feed_coord() {
    let arc = Arc::new(NzbgetRsFeedCoord::new());
    if let Ok(mut w) = arc.self_weak.lock() {
        *w = Arc::downgrade(&arc);
    }
    if let Some(lg) = crate::log::active_log() {
        lg.register_debuggable(2);
    }
    if let Ok(mut g) = ACTIVE_FEED_COORD.write() {
        *g = Some(Arc::downgrade(&arc));
    }
    feed_emit_log(&arc.effective_env(None), 0, &[b"Creating FeedCoordinator"]);
    if let Ok(mut g) = DAEMON_FEED_COORD.write() {
        *g = Some(arc);
    }
}

#[cfg(unix)]
pub(crate) fn start_daemon_feed_coord() {
    let Some(fc) = active_feed_coord() else {
        return;
    };
    let mut g = DAEMON_FEED_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_some() {
        return;
    }
    fc.daemon_stopped.store(false, Ordering::SeqCst);
    fc.daemon_running.store(true, Ordering::SeqCst);
    let fc_run = Arc::clone(&fc);
    if let Ok(h) = std::thread::Builder::new()
        .name("feed-coord".into())
        .spawn(move || {
            struct RunningReset(Arc<NzbgetRsFeedCoord>);
            impl Drop for RunningReset {
                fn drop(&mut self) {
                    self.0.daemon_running.store(false, Ordering::SeqCst);
                }
            }
            let _reset = RunningReset(Arc::clone(&fc_run));
            let _tg = crate::ffi::RustThreadGuard::enter(&fc_run.daemon_stopped);
            let env = fc_run.effective_env(None);
            fc_run.run_loop(&env);
        })
    {
        *g = Some(h);
    } else {
        fc.daemon_running.store(false, Ordering::SeqCst);
    }
}

#[cfg(unix)]
pub(crate) fn stop_daemon_feed_coord() {
    if let Some(fc) = active_feed_coord() {
        fc.daemon_stopped.store(true, Ordering::SeqCst);
        let env = fc.effective_env(None);
        fc.stop_downloads(&env);
    }
}

#[cfg(unix)]
pub(crate) fn is_daemon_feed_coord_running() -> bool {
    active_feed_coord().is_some_and(|fc| fc.daemon_running.load(Ordering::SeqCst))
}

#[cfg(unix)]
pub(crate) fn shutdown_daemon_feed_coord() {
    stop_daemon_feed_coord();
    let handle = DAEMON_FEED_HANDLE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(h) = handle {
        let _ = h.join();
    }
    if let Some(fc) = DAEMON_FEED_COORD.write().ok().and_then(|mut g| g.take()) {
        let env = fc.effective_env(None);
        fc.destroy_remaining(&env);
    }
    if let Ok(mut g) = ACTIVE_FEED_COORD.write() {
        *g = None;
    }
}

#[cfg(unix)]
pub(crate) fn notify_active_feed_queue_aspect(action: i32, url: Option<&[u8]>) {
    if let Some(fc) = active_feed_coord() {
        let env = fc.effective_env(None);
        fc.on_queue_update_bytes(&env, action, url.unwrap_or(&[]));
    }
}

#[cfg(unix)]
pub(crate) fn notify_active_feed_work_state() {
    if let Some(fc) = active_feed_coord() {
        fc.on_work_state_update();
    }
}

pub(crate) fn log_active_feed_debug_info() {
    if let Some(fc) = active_feed_coord() {
        let env = fc.effective_env(None);
        fc.log_debug_info(&env);
    }
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
            build_feed_dupe_key(b"", b"", b"", b"", 0, 439270, 0, 28369634, 2, 69, b"S02", b"E69"),
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
        assert_eq!(
            should_start_feed(15, 1000, 1200, false, 0, 1),
            (true, false)
        );
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

        let ep_no_series =
            build_feed_dupe_key(b"", b"", b"", b"", 0, 0, 0, 28369634, 2, 69, b"S02", b"E69");
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
            build_feed_dupe_key(b"", b"", b"", b"", 0, 0, 123456, 28369634, 2, 69, b"S02", b"E69"),
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
            (
                &b"FeedCategory"[..],
                0,
                &b"ItemCategory"[..],
                &b"OverrideCategory"[..],
                &b"OverrideCategory"[..],
            ),
            (
                &b"FeedCategory"[..],
                2,
                &b"ItemCategory"[..],
                &b""[..],
                &b"FeedCategory"[..],
            ),
            (&b""[..], 2, &b"ItemCategory"[..], &b""[..], &b""[..]),
            (&b"   "[..], 2, &b"   "[..], &b""[..], &b"   "[..]),
            (
                &b"FeedCategory"[..],
                1,
                &b"ItemCategory"[..],
                &b""[..],
                &b"FeedCategory"[..],
            ),
            (
                &b""[..],
                1,
                &b"ItemCategory"[..],
                &b""[..],
                &b"ItemCategory"[..],
            ),
            (&b""[..], 1, &b""[..], &b""[..], &b""[..]),
            (
                &b"FeedCategory"[..],
                0,
                &b"ItemCategory"[..],
                &b""[..],
                &b"FeedCategory"[..],
            ),
            (
                &b""[..],
                0,
                &b"ItemCategory"[..],
                &b""[..],
                &b"ItemCategory"[..],
            ),
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
