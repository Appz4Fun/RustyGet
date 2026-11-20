//! The article decoder (Decoder): finds yEnc or UU-encoded data in an
//! article as it arrives in pieces, decodes it into the caller's buffer and
//! checks it (sizes, CRC). yEnc bodies are decoded by rapidyenc (its decoder
//! and CRC functions are handed in). It matches the C++ Decoder, quirks
//! included: the line buffer appends as strncpy (a NUL ends the copied text
//! and the rest is zero-filled) and lines are found as strchr found them.

use std::ffi::{c_int, c_ulong};

mod scan;

/// Which break-scan kernel this CPU uses: 1 scalar, 2 AVX2, 3 AVX-512BW, 4 NEON.
pub fn scan_kernel() -> u8 {
    scan::imp::kernel()
}

/// How an incremental yEnc step ended (rapidyenc's RYDEC_END_*).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum YencEnd {
    /// No end sequence in this piece.
    More,
    /// "\r\n=y" was found; reading stopped after the 'y'.
    Control,
    /// "\r\n.\r\n" was found; reading stopped after it.
    Article,
}

/// Where a step stopped: reading at `src` and writing at `dst` (indexes into
/// the buffer it decoded).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct YencStep {
    pub end: YencEnd,
    pub src: usize,
    pub dst: usize,
}

/// rapidyenc_decode_incremental: decodes `buf[src..src + len]` into `buf`
/// from `dst`, carrying the line state across pieces in `state`. `dst` must
/// not be after `src`: each byte is written only after the byte it came from
/// was read, so the output may overwrite input already read.
pub type DecodeFn =
    fn(buf: &mut [u8], src: usize, len: usize, dst: usize, state: &mut c_int) -> YencStep;
/// rapidyenc_crc: the CRC-32 of `data` appended to `init`.
pub type CrcFn = fn(data: &[u8], init: u32) -> u32;

/// Built-in Rust incremental yEnc decoder powered by `nntmux_yenc` SIMD kernels
/// and exact NNTP dot-unstuffing / control-line framing state machine.
pub fn yenc_decode_incremental(
    buf: &mut [u8],
    src: usize,
    len: usize,
    dst: usize,
    state: &mut c_int,
) -> YencStep {
    if len == 0 {
        return YencStep { end: YencEnd::More, src, dst };
    }
    // indexes below are into `buf`, which ends where the input does
    let input_end = src.checked_add(len).expect("yEnc input range overflows");
    let buf = &mut buf[..input_end];
    let end = buf.len();
    let mut p = dst;
    let mut idx = src;
    let mut st = *state;

    macro_rules! at {
        ($i:expr) => {
            buf[$i]
        };
    }
    macro_rules! put {
        ($v:expr) => {{
            let v = $v;
            buf[p] = v;
            p += 1;
        }};
    }
    macro_rules! stop {
        ($end:expr, $read:expr, $new_state:expr) => {{
            *state = $new_state;
            return YencStep { end: $end, src: $read, dst: p };
        }};
    }
    macro_rules! check_end {
        ($new_state:expr) => {
            if idx == end {
                stop!(YencEnd::More, end, $new_state);
            }
        };
    }

    // 1. Resolve cross-chunk state from previous buffer
    loop {
        match st {
            6 => {
                // RYDEC_STATE_CRLFEQ
                if at!(idx) == b'y' {
                    stop!(YencEnd::Control, idx + 1, 3); // RYDEC_STATE_NONE
                }
                st = 1; // Fall through to RYDEC_STATE_EQ
            }
            1 => {
                // RYDEC_STATE_EQ
                let c = at!(idx);
                put!(c.wrapping_sub(42 + 64));
                idx += 1;
                if c != b'\r' {
                    break;
                }
                check_end!(2); // RYDEC_STATE_CR
                st = 2;
            }
            2 => {
                // RYDEC_STATE_CR
                if at!(idx) != b'\n' {
                    break;
                }
                idx += 1;
                check_end!(0); // RYDEC_STATE_CRLF
                st = 0;
            }
            0 => {
                // RYDEC_STATE_CRLF
                let c = at!(idx);
                if c == b'.' {
                    idx += 1;
                    check_end!(4); // RYDEC_STATE_CRLFDT
                    st = 4;
                } else if c == b'=' {
                    idx += 1;
                    check_end!(6); // RYDEC_STATE_CRLFEQ
                    st = 6;
                } else {
                    break;
                }
            }
            4 => {
                // RYDEC_STATE_CRLFDT
                let c = at!(idx);
                if c == b'\r' {
                    idx += 1;
                    check_end!(5); // RYDEC_STATE_CRLFDTCR
                    st = 5;
                } else if c == b'=' {
                    idx += 1;
                    check_end!(6); // RYDEC_STATE_CRLFEQ
                    st = 6;
                } else {
                    break;
                }
            }
            5 => {
                // RYDEC_STATE_CRLFDTCR
                if at!(idx) == b'\n' {
                    stop!(YencEnd::Article, idx + 1, 0); // RYDEC_STATE_CRLF
                }
                break;
            }
            _ => {
                // RYDEC_STATE_NONE
                break;
            }
        }
    }

    // 2. Main body loop with `nntmux_yenc` SIMD fast-path on unstuffed interior spans
    let mut next_simd_check = 0usize;
    while len >= 2 && idx < end - 2 {
        if idx >= next_simd_check {
            // Scan ahead for a span free of `\r\n.`, `\r\n=`, `=\r`, `=\n`
            let max_end = end - 2;
            let raw_scan_end = scan::find_break(buf, idx, max_end);
            let mut run_end = raw_scan_end;
            // Back off trailing `=`, `\r`, `\n` so the SIMD span ends in clean RYDEC_STATE_NONE
            while run_end > idx {
                let last = at!(run_end - 1);
                if last == b'=' || last == b'\r' || last == b'\n' {
                    run_end -= 1;
                } else {
                    break;
                }
            }
            let simd_len = run_end - idx;
            if simd_len >= 64 {
                if p != idx {
                    buf.copy_within(idx..run_end, p);
                }
                let written = nntmux_yenc::decoder::decode_in_place(&mut buf[p..p + simd_len]);
                p += written;
                idx = run_end;
                next_simd_check = raw_scan_end.max(idx + 1);
                if idx >= end - 2 {
                    break;
                }
            } else {
                next_simd_check = raw_scan_end.max(idx + 1);
            }
        }

        let c = at!(idx);
        match c {
            b'\r' => {
                if at!(idx + 1) == b'\n' {
                    let b2 = at!(idx + 2);
                    if b2 == b'.' {
                        idx += 3;
                        check_end!(4); // RYDEC_STATE_CRLFDT
                        let next = at!(idx);
                        if next == b'\r' {
                            idx += 1;
                            check_end!(5); // RYDEC_STATE_CRLFDTCR
                            if at!(idx) == b'\n' {
                                stop!(YencEnd::Article, idx + 1, 0); // RYDEC_STATE_CRLF
                            } else {
                                idx -= 1;
                            }
                        } else if next == b'=' {
                            idx += 1;
                            check_end!(6); // RYDEC_STATE_CRLFEQ
                            let esc = at!(idx);
                            if esc == b'y' {
                                stop!(YencEnd::Control, idx + 1, 3); // RYDEC_STATE_NONE
                            } else {
                                put!(esc.wrapping_sub(42 + 64));
                                if esc == b'\r' {
                                    idx -= 1;
                                }
                            }
                        } else {
                            idx -= 1;
                        }
                    } else if b2 == b'=' {
                        idx += 3;
                        check_end!(6); // RYDEC_STATE_CRLFEQ
                        let esc = at!(idx);
                        if esc == b'y' {
                            stop!(YencEnd::Control, idx + 1, 3); // RYDEC_STATE_NONE
                        } else {
                            put!(esc.wrapping_sub(42 + 64));
                            if esc == b'\r' {
                                idx -= 1;
                            }
                        }
                    }
                }
                idx += 1;
            }
            b'\n' => {
                idx += 1;
            }
            b'=' => {
                let esc = at!(idx + 1);
                put!(esc.wrapping_sub(42 + 64));
                idx += if esc != b'\r' { 2 } else { 1 };
            }
            _ => {
                put!(c.wrapping_sub(42));
                idx += 1;
            }
        }
    }

    *state = 3; // RYDEC_STATE_NONE

    // 2nd last char
    if len >= 2 && idx == end - 2 {
        let c = at!(idx);
        match c {
            b'\r' => {
                if at!(idx + 1) == b'\n' {
                    stop!(YencEnd::More, end, 0); // RYDEC_STATE_CRLF
                }
            }
            b'\n' => {}
            b'=' => {
                let esc = at!(idx + 1);
                put!(esc.wrapping_sub(42 + 64));
                if esc != b'\r' {
                    idx += 1;
                }
            }
            _ => {
                put!(c.wrapping_sub(42));
            }
        }
        idx += 1;
    }

    // Final char
    if idx + 1 == end {
        let c = at!(idx);
        if c != b'\n' && c != b'\r' && c != b'=' {
            put!(c.wrapping_sub(42));
        } else {
            *state = if c == b'=' {
                1 // RYDEC_STATE_EQ
            } else if c == b'\r' {
                2 // RYDEC_STATE_CR
            } else {
                3 // RYDEC_STATE_NONE
            };
        }
    }

    YencStep { end: YencEnd::More, src: end, dst: p }
}

/// Built-in Rust CRC-32 function powered by `crc32fast` SIMD kernels.
pub fn yenc_crc(data: &[u8], init: u32) -> u32 {
    crate::crc::crc32_append(data, init)
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Format {
    Unknown = 0,
    Yenc = 1,
    Ux = 2,
}

/// Check's results, as Decoder::EStatus.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    UnknownError = 0,
    Finished = 1,
    ArticleIncomplete = 2,
    CrcError = 3,
    InvalidSize = 4,
    NoBinaryData = 5,
}

/// StringBuilder as the decoder used it: `len` bytes count, the buffer
/// beyond them keeps what was there (strchr scans to the NUL Append wrote).
#[derive(Default)]
struct LineBuf {
    data: Vec<u8>,
    len: usize,
}

impl LineBuf {
    /// Append(str, len): strncpy, then a NUL after; a length of 0 means up to
    /// the NUL of `src` (it never is here: the caller passes the length).
    fn append(&mut self, src: &[u8]) {
        let end = self.len + src.len();
        if self.data.len() < end + 1 {
            self.data.resize(end + 1, 0);
        }
        let copy = find_byte(src, 0).unwrap_or(src.len());
        self.data[self.len..self.len + copy].copy_from_slice(&src[..copy]);
        self.data[self.len + copy..=end].fill(0);
        self.len = end;
    }

    fn set_len(&mut self, len: usize) {
        self.len = len;
    }

    /// The C string at `at` (up to its NUL; Append keeps one at `len`).
    fn c_str_end(&self, at: usize) -> usize {
        let limit = self.data.len();
        find_byte(&self.data[at.min(limit)..], 0).map_or(limit, |k| at + k)
    }
}

pub struct Decoder {
    decode_fn: DecodeFn,
    crc_fn: CrcFn,
    pub format: Format,
    begin: bool,
    part: bool,
    body: bool,
    end: bool,
    crc: bool,
    pub expected_crc: u32,
    pub calculated_crc: u32,
    pub begin_pos: i64,
    pub end_pos: i64,
    pub size: i64,
    end_size: i64,
    out_size: i64,
    pub eof: bool,
    pub crc_check: bool,
    state: c_int,
    pub raw_mode: bool,
    pub article_filename: Vec<u8>,
    line_buf: LineBuf,
    crc32: u32,
}

#[inline]
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn atoll(s: &[u8]) -> i64 {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut i = 0;
    while at(i) != 0 && is_c_space(at(i)) {
        i += 1;
    }
    let mut neg = false;
    if matches!(at(i), b'+' | b'-') {
        neg = at(i) == b'-';
        i += 1;
    }
    let mut acc: i64 = 0;
    while at(i).is_ascii_digit() {
        let d = (at(i) - b'0') as i64;
        acc = if neg {
            acc.saturating_mul(10).saturating_sub(d)
        } else {
            acc.saturating_mul(10).saturating_add(d)
        };
        i += 1;
    }
    acc
}

fn strtoul_hex(s: &[u8]) -> u32 {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut i = 0;
    while at(i) != 0 && is_c_space(at(i)) {
        i += 1;
    }
    let mut neg = false;
    if matches!(at(i), b'+' | b'-') {
        neg = at(i) == b'-';
        i += 1;
    }
    if at(i) == b'0' && (at(i + 1) | 0x20) == b'x' && at(i + 2).is_ascii_hexdigit() {
        i += 2;
    }
    let mut acc: c_ulong = 0;
    let mut overflow = false;
    while let Some(d) = (at(i) as char).to_digit(16) {
        if let Some(next) = acc
            .checked_mul(16)
            .and_then(|v| v.checked_add(d as c_ulong))
        {
            acc = next;
        } else {
            overflow = true;
        }
        i += 1;
    }
    let res = if overflow {
        c_ulong::MAX
    } else if neg {
        acc.wrapping_neg()
    } else {
        acc
    };
    res as u32
}

use crate::bytes::{find, find_byte};

/// strstr on the C string at the start of `s`: the first `key` with no NUL
/// before it. `s` may run on past its NUL: a found key is checked instead of
/// measuring the string first, so a present key costs what strstr spent.
fn cfind(s: &[u8], key: &[u8]) -> Option<usize> {
    let k = find(s, key)?;
    find_byte(&s[..k], 0).is_none().then_some(k)
}

/// UU_DECODE_CHAR.
fn uu(c: u8) -> u8 {
    if c == b'`' {
        0
    } else {
        c.wrapping_sub(b' ') & 0o77
    }
}

impl Decoder {
    pub fn new(decode_fn: DecodeFn, crc_fn: CrcFn) -> Decoder {
        let mut d = Decoder {
            decode_fn,
            crc_fn,
            format: Format::Unknown,
            begin: false,
            part: false,
            body: false,
            end: false,
            crc: false,
            expected_crc: 0,
            calculated_crc: 0,
            begin_pos: 0,
            end_pos: 0,
            size: 0,
            end_size: 0,
            out_size: 0,
            eof: false,
            crc_check: false,
            state: 0,
            raw_mode: false,
            article_filename: Vec::new(),
            line_buf: LineBuf::default(),
            crc32: 0,
        };
        d.clear();
        d
    }

    /// Decoder::Clear (the format and raw mode stay, as in the C++).
    pub fn clear(&mut self) {
        self.article_filename.clear();
        self.body = false;
        self.begin = false;
        self.part = false;
        self.end = false;
        self.crc = false;
        self.eof = false;
        self.expected_crc = 0;
        self.crc32 = 0;
        self.begin_pos = 0;
        self.end_pos = 0;
        self.size = 0;
        self.end_size = 0;
        self.out_size = 0;
        self.state = 0;
        self.crc_check = false;
        if self.line_buf.data.len() < 1024 * 8 + 1 {
            self.line_buf.data.resize(1024 * 8 + 1, 0);
        }
        self.line_buf.set_len(0);
    }

    /// Decoder::DecodeBuffer: decodes the first `len` bytes of `buf`, writing
    /// the data to the start of `buf` itself; returns its length.
    ///
    /// In line mode, zero length means the NUL-terminated string at the start
    /// of `buf` (StringBuilder::Append's default). `buf` must also fit the
    /// decoded output: up to the input length plus 63 bytes for a pending UU
    /// line (Connection reserves 128 slack bytes); a shorter one panics.
    pub fn decode_buffer(&mut self, buf: &mut [u8], len: usize) -> usize {
        if self.raw_mode {
            self.process_raw(&buf[..len]);
            return len;
        }
        let mut outlen = 0usize;
        if self.body && self.format == Format::Yenc {
            let decode = self.decode_fn;
            let step = decode(buf, 0, len, 0, &mut self.state);
            let tail = buf[step.src..len].to_vec();
            outlen = self.yenc_done(step.end, &tail, &buf[..step.dst]);
            if self.body {
                return outlen;
            }
        } else {
            let chunk = if len == 0 {
                crate::cstr::until_nul(buf)
            } else {
                &buf[..len]
            };
            self.line_buf.append(chunk);
        }

        let mut line = 0usize;
        // strchr(line, '\n') stops at the first newline or NUL.
        while let Some(nl) = self.line_buf.data[line..]
            .iter()
            .position(|&b| b == 0 || b == b'\n')
            .map(|k| line + k)
        {
            if self.line_buf.data[nl] == 0 {
                break;
            }
            let llen = nl - line + 1;
            let lb = &self.line_buf.data;
            if lb[line] == b'.' && lb.get(line + 1) == Some(&b'\r') {
                self.eof = true;
                self.line_buf.set_len(0);
                return outlen;
            }
            if self.format == Format::Unknown {
                self.format = detect_format(&self.line_buf.data[line..], llen);
            }
            if self.format == Format::Yenc {
                // Only control lines use strstr, which looked past the newline.
                // Ordinary lines must not scan/copy the whole remaining article.
                let text = &self.line_buf.data[line..];
                if text.starts_with(b"=ybegin ")
                    || text.starts_with(b"=ypart ")
                    || text.starts_with(b"=yend ")
                {
                    // the line's C string runs on past its newline (strstr looked there);
                    // read it in place instead of copying the rest of the piece
                    let end = self.line_buf.len.max(line);
                    let line_buf = std::mem::take(&mut self.line_buf);
                    self.process_yenc(&line_buf.data[line..end], llen);
                    self.line_buf = line_buf;
                }
                if self.body {
                    // decode the rest in place in the line buffer, then copy it out
                    let rest_at = nl + 1;
                    let rest_end = self.line_buf.len;
                    let decode = self.decode_fn;
                    let data = &mut self.line_buf.data;
                    let step = decode(data, rest_at, rest_end - rest_at, rest_at, &mut self.state);
                    let written = step.dst - rest_at;
                    buf[..written].copy_from_slice(&data[rest_at..step.dst]);
                    let tail = data[step.src..rest_end].to_vec();
                    outlen = self.yenc_done(step.end, &tail, &buf[..written]);
                    if self.body {
                        self.line_buf.set_len(0);
                        return outlen;
                    }
                    line = 0;
                    continue;
                }
            } else if self.format == Format::Ux {
                // Only a begin line's filename parser scans past its newline.
                // Copying the entire remaining article per UU line is quadratic.
                let end = if self.line_buf.data[line..].starts_with(b"begin ") && !self.body {
                    self.line_buf.c_str_end(line) + 1
                } else {
                    (line + llen.max(63)).min(self.line_buf.data.len())
                };
                let text = self.line_buf.data[line..end].to_vec();
                outlen += self.decode_ux(&text, llen, &mut buf[outlen..]);
            }
            line = nl + 1;
        }
        if self.line_buf.data.get(line).is_some_and(|&b| b != 0) {
            let rest = self.line_buf.len - line;
            self.line_buf.data.copy_within(line..line + rest, 0);
            self.line_buf.set_len(rest);
        } else {
            self.line_buf.set_len(0);
        }
        outlen
    }

    fn parse_ypart(&mut self, line: &[u8]) {
        self.part = true;
        self.body = true;
        if let Some(k) = cfind(line, b" begin=") {
            self.begin_pos = atoll(&line[k + 7..]);
        }
        if let Some(k) = cfind(line, b" end=") {
            self.end_pos = atoll(&line[k + 5..]);
        }
    }

    /// ParseName: the name after " name=" up to the line end, or to a
    /// "=ypart " on the same line (which is parsed, looking past the line as
    /// the C++ did). An empty name was CString(pb, 0): the rest of the C string.
    fn parse_name(&mut self, full: &[u8], len: usize, at: usize) {
        let pb = at + 6;
        let mut pe = pb;
        while pe < len && !matches!(full[pe], 0 | b'\n' | b'\r') {
            if pe + 7 <= len && &full[pe..pe + 7] == b"=ypart " {
                self.parse_ypart(&full[pe..]);
                break;
            }
            pe += 1;
        }
        let name = if pe > pb {
            &full[pb..pe]
        } else {
            &full[pb.min(full.len())..]
        };
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        let mut out = Vec::new();
        crate::text::latin1_to_utf8(name, &mut out);
        self.article_filename = out;
    }

    fn process_yenc(&mut self, full: &[u8], len: usize) {
        let line = &full[..len.min(full.len())];
        let cs = full;
        if len >= 8 && line.starts_with(b"=ybegin ") {
            self.begin = true;
            if let Some(k) = cfind(cs, b" size=").filter(|&k| k + 6 < len) {
                self.size = atoll(&cs[k + 6..]);
            }
            self.part = cfind(cs, b" part=").is_some_and(|k| k < len);
            if !self.part {
                self.body = true;
                self.begin_pos = 1;
                self.end_pos = self.size;
            }
            if let Some(k) = cfind(cs, b" name=").filter(|&k| k + 6 < len) {
                self.parse_name(full, len, k);
            }
        } else if len >= 7 && line.starts_with(b"=ypart ") {
            self.parse_ypart(cs);
        } else if len >= 6 && line.starts_with(b"=yend ") {
            self.end = true;
            let (key, offset): (&[u8], usize) = if self.part {
                (b" pcrc32=", 8)
            } else {
                (b" crc32=", 7)
            };
            if let Some(k) = cfind(cs, key).filter(|&k| k + offset < len) {
                self.crc = true;
                self.expected_crc = strtoul_hex(&cs[k + offset..]);
            }
            if let Some(k) = cfind(cs, b" size=").filter(|&k| k + 6 < len) {
                self.end_size = atoll(&cs[k + 6..]);
            }
        }
    }

    /// DecodeYenc after rapidyenc wrote `out`: at the end of the yEnc data
    /// the rest (`tail`, the input after the end sequence) goes back to the
    /// line buffer. Returns the length written.
    fn yenc_done(&mut self, end: YencEnd, tail: &[u8], out: &[u8]) -> usize {
        if end != YencEnd::More {
            self.line_buf.set_len(0);
            self.line_buf
                .append(if end == YencEnd::Control { b"=y" } else { b".\r\n" });
            if !tail.is_empty() {
                self.line_buf.append(tail);
            }
            self.body = false;
        }
        if self.crc_check {
            self.crc32 = (self.crc_fn)(out, self.crc32);
        }
        self.out_size += out.len() as i64;
        out.len()
    }

    /// DecodeUx for one line (`len` bytes of `line`, which goes on to its NUL).
    fn decode_ux(&mut self, line: &[u8], len: usize, out: &mut [u8]) -> usize {
        let at = |k: usize| *line.get(k).unwrap_or(&0);
        if !self.body {
            if line.starts_with(b"begin ") {
                let mut pb = 6;
                while !matches!(at(pb), b' ' | 0 | b'\n' | b'\r') {
                    pb += 1;
                }
                pb += 1;
                let mut pe = pb;
                while !matches!(at(pe), 0 | b'\n' | b'\r') {
                    pe += 1;
                }
                // an empty name was CString(pb, 0): the rest of the C string
                let name = if pe > pb {
                    line.get(pb..pe).unwrap_or_default()
                } else {
                    line.get(pb..).unwrap_or_default()
                };
                let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
                let mut outn = Vec::new();
                crate::text::latin1_to_utf8(name, &mut outn);
                self.article_filename = outn;
                self.body = true;
                return 0;
            } else if (len == 62 || len == 63) && matches!(at(62), b'\n' | b'\r') && at(0) == b'M' {
                self.body = true;
            }
        }
        if self.body && (line.starts_with(b"end ") || at(0) == b'`') {
            self.end = true;
        }
        if self.body && !self.end {
            let eff = uu(at(0)) as usize;
            if eff > len {
                return 0;
            }
            // a line shorter than its length says: padded with spaces
            let mut data_len = len;
            while data_len > 0 && matches!(at(data_len - 1), b'\n' | b'\r') {
                data_len -= 1;
            }
            let need = 1 + 4 * eff.div_ceil(3);
            let mut padded = [b' '; 1 + 4 * 22];
            let src: Vec<u8> = if data_len < need {
                padded[..data_len].copy_from_slice(&line[..data_len]);
                padded.to_vec()
            } else {
                line[..len.max(need).min(line.len())].to_vec()
            };
            let g = |k: usize| uu(*src.get(k).unwrap_or(&0));
            let mut o = 0usize;
            let mut i = 1usize;
            let mut eff = eff as isize;
            while eff > 0 {
                if eff >= 3 {
                    out[o] = g(i) << 2 | g(i + 1) >> 4;
                    out[o + 1] = g(i + 1) << 4 | g(i + 2) >> 2;
                    out[o + 2] = g(i + 2) << 6 | g(i + 3);
                    o += 3;
                } else {
                    out[o] = g(i) << 2 | g(i + 1) >> 4;
                    o += 1;
                    if eff >= 2 {
                        out[o] = g(i + 1) << 4 | g(i + 2) >> 2;
                        o += 1;
                    }
                }
                i += 4;
                eff -= 3;
            }
            return o;
        }
        0
    }

    fn process_raw(&mut self, b: &[u8]) {
        let len = b.len();
        // Cross-buffer completion needs at most four leading bytes; retain
        // the bulk search for markers wholly inside the current buffer.
        let mut prefix = self.state;
        if matches!(self.state, 1..=4) {
            self.eof = false;
        }
        for &byte in b.iter().take(4) {
            prefix = match (prefix, byte) {
                (1, b'\n') => 2,
                (2, b'.') => 3,
                (3, b'\r') => 4,
                (4, b'\n') => {
                    self.eof = true;
                    2
                }
                (_, b'\r') => 1,
                _ => 0,
            };
        }
        self.eof |= find(b, b"\r\n.\r\n").is_some();
        // Preserve historical EOF/empty-call policy once complete. Only
        // unfinished positive short reads need the cumulative prefix.
        if len > 0 && len < 4 && !self.eof {
            self.state = prefix;
            return;
        }
        self.state = if b.ends_with(b"\r\n.\r") {
            4
        } else if b.ends_with(b"\r\n.") {
            3
        } else if b.ends_with(b"\r\n") {
            2
        } else if b.ends_with(b"\r") {
            1
        } else {
            0
        };
    }

    /// Decoder::Check.
    pub fn check(&mut self) -> Status {
        match self.format {
            Format::Yenc => {
                self.calculated_crc = self.crc32;
                if !self.begin {
                    Status::NoBinaryData
                } else if !self.end {
                    Status::ArticleIncomplete
                } else if (!self.part && self.size != self.end_size)
                    || self.end_size != self.out_size
                {
                    Status::InvalidSize
                } else if self.crc_check && self.crc && self.expected_crc != self.calculated_crc {
                    Status::CrcError
                } else {
                    Status::Finished
                }
            }
            Format::Ux => {
                if self.body {
                    Status::Finished
                } else {
                    Status::NoBinaryData
                }
            }
            Format::Unknown => Status::UnknownError,
        }
    }
}

/// DetectFormat for a line (`len` bytes; the buffer goes on to its NUL).
fn detect_format(line: &[u8], len: usize) -> Format {
    let at = |k: usize| *line.get(k).unwrap_or(&0);
    if line.starts_with(b"=ybegin ") {
        return Format::Yenc;
    }
    if (len == 62 || len == 63) && matches!(at(62), b'\n' | b'\r') && at(0) == b'M' {
        return Format::Ux;
    }
    if line.starts_with(b"begin ") {
        let mut i = 6;
        while at(i) != 0 && at(i) != b' ' {
            if !(b'0'..=b'7').contains(&at(i)) {
                return Format::Unknown;
            }
            i += 1;
        }
        return Format::Ux;
    }
    Format::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(buf: &mut [u8], src: usize, len: usize, dst: usize, _state: &mut c_int) -> YencStep {
        buf.copy_within(src..src + len, dst);
        YencStep { end: YencEnd::More, src: src + len, dst: dst + len }
    }

    fn crc(_data: &[u8], init: u32) -> u32 {
        init
    }

    #[test]
    fn control_key_search_matches_strstr() {
        for key in [
            b" size=".as_slice(),
            b" part=",
            b" name=",
            b" begin=",
            b" end=",
            b" crc32=",
            b" pcrc32=",
        ] {
            let text = [b"=ybegin x".as_slice(), key, b"1\r\n", key, b"2"].concat();
            for len in 0..=text.len() {
                for nul in 0..=len {
                    let mut s = text[..len].to_vec();
                    s.push(0);
                    s[nul] = 0;
                    let c_slice = &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())];
                    let expected = c_slice.windows(key.len()).position(|w| w == key);
                    assert_eq!(
                        cfind(&s[..len], key),
                        expected,
                        "len={len}, NUL={nul}, key={key:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn append_matches_copy_and_zero_padding() {
        let mut buf = LineBuf::default();
        let mut expected = Vec::new();
        let mut len = 0;
        for size in [0, 1, 7, 8192, 8193] {
            for nul in [0, size / 2, size] {
                let mut src = vec![b'x'; size];
                if nul < size {
                    src[nul] = 0;
                }
                let end = len + size;
                expected.resize(expected.len().max(end + 1), 0);
                expected[len..len + nul].copy_from_slice(&src[..nul]);
                expected[len + nul..=end].fill(0);
                buf.append(&src);
                assert_eq!(buf.data, expected);
                assert_eq!(buf.len, end);
                // SetLength preserves stale bytes; the next append must still
                // replace its terminator and zero-pad only the appended range.
                len = end / 2;
                buf.set_len(len);
            }
        }
    }

    #[test]
    fn control_lines_keep_c_string_and_line_bounds() {
        let mut d = Decoder::new(decode, crc);
        let full = b"=ybegin part=1 size=\n42 name=late\0 size=99";
        let len = full.iter().position(|&c| c == b'\n').unwrap() + 1;
        d.process_yenc(full, len);
        assert_eq!(d.size, 42); // atoll can skip the newline
        assert!(d.part);
        assert!(d.article_filename.is_empty()); // name key starts after len
        d.size = 17;
        d.process_yenc(full, len - 1);
        assert_eq!(d.size, 17); // k + 6 == len is rejected
        d.process_yenc(b"=ybegin \n part=1 size=99 name=late", 9);
        assert!(!d.part); // k == len is rejected
        d.parse_ypart(b"=ypart begin=12\0 end=99");
        assert_eq!(d.begin_pos, 12);
        assert_eq!(d.end_pos, 17); // end after NUL is invisible
        d.parse_ypart(b"=ypart begin=\0");
        assert_eq!(d.begin_pos, 0);

        let full = b"=ybegin part=1 name=\r\nrest\0ignored";
        d.process_yenc(full, 21);
        assert_eq!(d.article_filename, b"\r\nrest");
        let full = b"=ybegin name==ypart begin=7\r\n end=9\0 end=99";
        d.process_yenc(full, 28);
        assert_eq!(d.begin_pos, 7);
        assert_eq!(d.end_pos, 9);
        assert_eq!(d.article_filename, b"=ypart begin=7\r\n end=9");
        assert_eq!(atoll(b" \t-12\0"), -12);
        assert_eq!(atoll(b"\0 99"), 0);
        assert_eq!(strtoul_hex(b"0x\0ff"), 0);
        assert_eq!(strtoul_hex(b"-0xf\0ff"), 0u32.wrapping_sub(15));
    }

    // Exercise DecodeBuffer, retaining the caller's 128-byte slack and canaries.
    fn check_raw_chunk(d: &mut Decoder, chunk: &[u8], eof: bool) {
        let mut buffer = vec![0x5a; 32 + chunk.len() + 128 + 32];
        buffer[32..32 + chunk.len()].copy_from_slice(chunk);
        let before = buffer.clone();
        assert_eq!(
            d.decode_buffer(&mut buffer[32..], chunk.len()),
            chunk.len()
        );
        assert_eq!(buffer, before);
        assert_eq!(d.eof, eof, "chunk={chunk:?}");
    }

    fn captured_raw_wire() -> Vec<u8> {
        // Exact retained Stage2 wire; the one-byte test uses a different schedule.
        let mut wire = b"fixture raw\r\n".to_vec();
        wire.extend_from_slice(&[b'R'; 110]);
        wire.extend_from_slice(b"\r\n.\r\n");
        wire
    }

    #[test]
    fn raw_terminator_partitions() {
        let marker = b"\r\n.\r\n";
        for splits in 0..16 {
            let mut d = Decoder::new(decode, crc);
            d.raw_mode = true;
            let mut start = 0;
            for end in 1..=marker.len() {
                if end == marker.len() || splits & (1 << (end - 1)) != 0 {
                    check_raw_chunk(&mut d, &marker[start..end], end == marker.len());
                    start = end;
                }
            }
        }
    }

    #[test]
    fn raw_captured_fragment_schedule() {
        let wire = captured_raw_wire();
        assert_eq!(wire.len(), 128);
        let mut d = Decoder::new(decode, crc);
        d.raw_mode = true;
        let mut offset = 0;
        for len in [1, 6, 17, 17, 17, 17, 17, 17, 15, 1, 1, 1, 1] {
            check_raw_chunk(
                &mut d,
                &wire[offset..offset + len],
                offset + len == wire.len(),
            );
            offset += len;
        }
        assert_eq!(offset, wire.len());
    }

    #[test]
    fn raw_one_byte_chunks() {
        let wire = captured_raw_wire();
        let mut d = Decoder::new(decode, crc);
        d.raw_mode = true;
        for i in 0..wire.len() {
            check_raw_chunk(&mut d, &wire[i..i + 1], i + 1 == wire.len());
        }
    }

    #[test]
    fn raw_prefix_overlap() {
        let sequences: &[&[&[u8]]] = &[
            &[b"\r", b"\r", b"\n", b".", b"\r", b"\n"],
            &[b"\r\n", b"\r", b"\n", b".", b"\r", b"\n"],
            &[b"\r\n.\r", b"\r", b"\n", b".", b"\r", b"\n"],
        ];
        for chunks in sequences {
            let mut d = Decoder::new(decode, crc);
            d.raw_mode = true;
            for (i, chunk) in chunks.iter().enumerate() {
                check_raw_chunk(&mut d, chunk, i + 1 == chunks.len());
            }
        }
        let mut d = Decoder::new(decode, crc);
        d.raw_mode = true;
        check_raw_chunk(&mut d, b"\r\n.\r\n", true);
        check_raw_chunk(&mut d, b".\r\n", true);
    }

    #[test]
    fn raw_near_miss() {
        let wires: &[&[u8]] = &[
            b"\r\n..\r\n",
            b"\n.\r\n",
            b"\r\n.\n",
            b"\r.\r\n",
            b"\r\n.x\r\n",
            b"\r\n.\rX",
            b"\0\r\n.\0\r\n",
        ];
        for wire in wires {
            let mut d = Decoder::new(decode, crc);
            d.raw_mode = true;
            for chunk in wire.chunks(1) {
                check_raw_chunk(&mut d, chunk, false);
            }
        }
    }

    #[test]
    fn raw_zero_length_policy() {
        let prefixes: &[&[u8]] = &[b"", b"\r", b"\r\n", b"\r\n.\r"];
        let continuations: &[&[u8]] = &[b"x", b"\n.\r\n", b".\r\n", b"\n"];
        for i in 0..4 {
            let mut d = Decoder::new(decode, crc);
            d.raw_mode = true;
            check_raw_chunk(&mut d, b"\r\n.\r\nx", true);
            if i != 0 {
                check_raw_chunk(&mut d, prefixes[i], true);
            }
            check_raw_chunk(&mut d, b"", i == 0);
            check_raw_chunk(&mut d, continuations[i], i == 0);
            check_raw_chunk(&mut d, b"\r\n.\r\n", true);
        }
    }

    #[test]
    fn raw_clear_reuse() {
        let marker = b"\r\n.\r\n";
        for prefix in 1..=4 {
            let mut d = Decoder::new(decode, crc);
            let mut header = b"begin 644 f.bin\r\n".to_vec();
            let n = header.len();
            assert_eq!(d.decode_buffer(&mut header, n), 0);
            assert_eq!(d.format, Format::Ux);
            d.raw_mode = true;
            check_raw_chunk(&mut d, &marker[..prefix], false);
            d.clear();
            assert!(!d.eof);
            assert_eq!(d.format, Format::Ux);
            assert!(d.raw_mode);
            assert_eq!(d.state, 0);
            check_raw_chunk(&mut d, &marker[prefix..], false);
            check_raw_chunk(&mut d, marker, true);
        }
    }

    #[test]
    fn raw_bulk_working_buffers() {
        let mut large = vec![0x80; 65536 - 5];
        large.extend_from_slice(b"\r\n.\r\n");
        for wire in [captured_raw_wire(), large] {
            let mut d = Decoder::new(decode, crc);
            d.raw_mode = true;
            check_raw_chunk(&mut d, &wire, true);
        }
    }

    #[test]
    fn raw_legacy_eof_policy() {
        let sequences: &[(&[&[u8]], &[bool], &[c_int])] = &[
            (
                &[b"\r\n.\r\nx", b"more", b"\r", b"x"],
                &[true, true, true, false],
                &[0, 0, 1, 0],
            ),
            (&[b"\r\n.\r", b"\n", b"x"], &[false, true, true], &[4, 0, 0]),
            (
                &[b"\r\n.", b"\r\n", b"x"],
                &[false, true, false],
                &[3, 2, 0],
            ),
        ];
        for &(chunks, expected, states) in sequences {
            let mut d = Decoder::new(decode, crc);
            d.raw_mode = true;
            for i in 0..chunks.len() {
                check_raw_chunk(&mut d, chunks[i], expected[i]);
                assert_eq!(d.state, states[i]);
            }
        }
    }

    #[test]
    fn raw_eof_state_zero_is_sticky() {
        let mut d = Decoder::new(decode, crc);
        d.process_raw(b"\r\n.\r\nx");
        d.process_raw(b"more");
        assert!(d.eof);
        d.process_raw(b"\r");
        assert!(d.eof);
        d.process_raw(b"x");
        assert!(!d.eof);
    }

    #[test]
    fn numeric_crt_overflow_and_sign() {
        assert_eq!(strtoul_hex(b"-10000000000000000"), u32::MAX);
        assert_eq!(strtoul_hex(b"-fffffffffffffffff"), u32::MAX);
        assert_eq!(strtoul_hex(b" \t-0x1\r\n"), u32::MAX);
        assert_eq!(
            strtoul_hex(b"100000000"),
            if std::mem::size_of::<c_ulong>() == 4 {
                u32::MAX
            } else {
                0
            }
        );
        assert_eq!(atoll(b" \t\x0b-123rest"), -123);
    }

    #[test]
    fn zero_length_line_and_filename() {
        let mut d = Decoder::new(decode, crc);
        assert_eq!(d.article_filename, b"");
        let mut b = [0u8; 256];
        let input = b"begin 644 x\r\n#04)#\r\n`\r\n\0";
        b[..input.len()].copy_from_slice(input);
        assert_eq!(d.decode_buffer(&mut b, 0), 3);
        assert_eq!(&b[..3], b"ABC");
        assert_eq!(d.article_filename, b"x");
    }

    fn test_yenc_decode(
        buf: &mut [u8],
        src: usize,
        len: usize,
        dst: usize,
        _state: &mut c_int,
    ) -> YencStep {
        let mut written = dst;
        let mut i = src;
        let end = src + len;
        let mut endseq = YencEnd::More;
        while i < end {
            if i + 4 <= end && &buf[i..i + 4] == b"\r\n=y" {
                i += 4;
                endseq = YencEnd::Control;
                break;
            }
            if i + 5 <= end && &buf[i..i + 5] == b"\r\n.\r\n" {
                i += 5;
                endseq = YencEnd::Article;
                break;
            }
            let b = buf[i];
            i += 1;
            if b == b'\r' || b == b'\n' {
                continue;
            }
            buf[written] = b.wrapping_sub(42);
            written += 1;
        }
        YencStep { end: endseq, src: i, dst: written }
    }

    fn test_yenc_crc(data: &[u8], init: u32) -> u32 {
        let mut acc = crate::crc::Crc32::new();
        acc.append(data);
        crate::crc::combine(init, acc.finish(), data.len() as u32)
    }

    fn yenc_encode(data: &[u8]) -> Vec<u8> {
        data.iter().map(|&c| c.wrapping_add(42)).collect()
    }

    #[test]
    fn decoder_yenc_and_uu_full_suite_port_from_cpp() {
        // 1. SingleMessageTest
        {
            let mut decoder = Decoder::new(test_yenc_decode, test_yenc_crc);
            decoder.crc_check = true;
            let data = b"nzbget";
            let encoded = yenc_encode(data);
            let mut msg = Vec::new();
            msg.extend_from_slice(
                format!("=ybegin line=128 size={} name=name.dat\r\n", data.len()).as_bytes(),
            );
            msg.extend_from_slice(&encoded);
            msg.extend_from_slice(
                format!("\r\n=yend size={} crc32=a30bff0d\r\n.\r\n", data.len()).as_bytes(),
            );
            let n = msg.len();
            let len = decoder.decode_buffer(&mut msg, n);
            assert_eq!(decoder.check(), Status::Finished);
            assert_eq!(decoder.article_filename, b"name.dat");
            assert_eq!(decoder.size, 6);
            assert_eq!(decoder.calculated_crc, 0xa30bff0d);
            assert_eq!(decoder.format, Format::Yenc);
            assert!(decoder.eof);
            assert_eq!(&msg[..len], b"nzbget");
        }

        // 2. MultipartMessageTest & 3. UnusualMultipartMessageTest
        for unusual in [false, true] {
            let mut decoder = Decoder::new(test_yenc_decode, test_yenc_crc);
            decoder.crc_check = true;
            let data1 = b"nzb";
            let data2 = b"get";
            let enc1 = yenc_encode(data1);
            let enc2 = yenc_encode(data2);
            let total_size = data1.len() + data2.len();
            let sep = if unusual { "" } else { "\r\n" };

            let mut msg1 = Vec::new();
            msg1.extend_from_slice(
                format!(
                    "=ybegin part=1 total=2 line=128 size={total_size} name=name.dat{sep}=ypart begin=1 end={}\r\n",
                    data1.len()
                )
                .as_bytes(),
            );
            msg1.extend_from_slice(&enc1);
            msg1.extend_from_slice(
                format!("\r\n=yend size={} pcrc32=cb64ae30\r\n.\r\n", data1.len()).as_bytes(),
            );

            let mut msg2 = Vec::new();
            msg2.extend_from_slice(
                format!(
                    "=ybegin part=2 total=2 line=128 size={total_size} name=name.dat{sep}=ypart begin={} end={total_size}\r\n",
                    data1.len() + 1
                )
                .as_bytes(),
            );
            msg2.extend_from_slice(&enc2);
            msg2.extend_from_slice(
                format!(
                    "\r\n=yend size={} pcrc32=fd3b2e70 crc32=a30bff0d\r\n.\r\n",
                    data2.len()
                )
                .as_bytes(),
            );

            let n = msg1.len();
            let len1 = decoder.decode_buffer(&mut msg1, n);
            assert_eq!(decoder.check(), Status::Finished);
            assert_eq!(decoder.article_filename, b"name.dat");
            assert_eq!(decoder.size, total_size as i64);
            assert_eq!(decoder.calculated_crc, 0xcb64ae30);
            assert_eq!(decoder.format, Format::Yenc);
            assert!(decoder.eof);
            assert_eq!(&msg1[..len1], b"nzb");

            decoder.clear();
            decoder.crc_check = true;
            let n = msg2.len();
            let len2 = decoder.decode_buffer(&mut msg2, n);
            assert_eq!(decoder.article_filename, b"name.dat");
            assert_eq!(decoder.check(), Status::Finished);
            assert_eq!(len1, data2.len());
            assert_eq!(len1 + len2, total_size);
            assert_eq!(decoder.size, total_size as i64);
            assert_eq!(decoder.calculated_crc, 0xfd3b2e70);
            assert!(decoder.eof);
            assert_eq!(&msg2[..len2], b"get");
        }

        // 4. BufferOverflowTest
        {
            let mut decoder = Decoder::new(decode, crc);
            decoder.raw_mode = true;
            let mut buf = vec![b'x'; 4096];
            let len = decoder.decode_buffer(&mut buf, 4096);
            assert_eq!(len, 4096);
            assert!(!decoder.eof);
        }

        // 5. UuDecodeOutputBoundTest
        {
            let mut decoder = Decoder::new(decode, crc);
            let mut first = b"begin 644 f.bin\r\n_".to_vec();
            first.extend_from_slice(&[b'A'; 62]);
            let first_len = first.len();
            first.resize(first_len + 128, 0);
            decoder.decode_buffer(&mut first, first_len);

            let mut next = vec![b'A'; 22];
            next.extend_from_slice(b"\r\n");
            while next.len() < 512 - 87 {
                next.push(b'_');
                next.extend_from_slice(&[b'A'; 84]);
                next.extend_from_slice(b"\r\n");
            }
            let next_len = next.len();
            let mut second = vec![b'Z'; next_len + 128];
            second[..next_len].copy_from_slice(&next);
            let outlen = decoder.decode_buffer(&mut second, next_len);
            assert!(outlen > 0);
            assert!(outlen <= next_len + 63);
        }

        // 6. UuDecodeShortLineTest
        {
            let decode_uu_line = |line: &[u8]| -> Vec<u8> {
                let mut decoder = Decoder::new(decode, crc);
                let mut text = b"begin 644 f.bin\r\n".to_vec();
                text.extend_from_slice(line);
                text.extend_from_slice(b"\r\n");
                let text_len = text.len();
                let mut buffer = vec![b'Z'; text_len + 128];
                buffer[..text_len].copy_from_slice(&text);
                let outlen = decoder.decode_buffer(&mut buffer, text_len);
                buffer[..outlen].to_vec()
            };
            let mut short_line = vec![b'M'];
            short_line.extend_from_slice(&[b'!'; 46]);
            let mut full_line = short_line.clone();
            full_line.extend_from_slice(&[b' '; 14]);
            let got = decode_uu_line(&short_line);
            assert_eq!(got.len(), 45);
            assert_eq!(got, decode_uu_line(&full_line));
        }
    }

    /// Encode `data` as a news server sends a yEnc part: 128-byte lines,
    /// critical bytes escaped, and a line-leading '.' either escaped or (as
    /// NNTP dot-stuffing) doubled.
    fn yenc_article(data: &[u8], stuff_dots: bool) -> Vec<u8> {
        let size = data.len();
        let mut out =
            format!("=ybegin part=1 line=128 size={size} name=f.bin\r\n=ypart begin=1 end={size}\r\n")
                .into_bytes();
        let mut col = 0;
        for &b in data {
            let e = b.wrapping_add(42);
            let dot_lead = col == 0 && e == b'.';
            let esc = matches!(e, 0 | b'\n' | b'\r' | b'=')
                || (col == 0 && matches!(e, b'\t' | b' '))
                || (dot_lead && !stuff_dots);
            if esc {
                out.extend_from_slice(&[b'=', e.wrapping_add(64)]);
                col += 2;
            } else {
                if dot_lead {
                    out.push(b'.');
                }
                out.push(e);
                col += 1;
            }
            if col >= 128 {
                out.extend_from_slice(b"\r\n");
                col = 0;
            }
        }
        let crc = crc32fast::hash(data);
        out.extend_from_slice(
            format!("\r\n=yend size={size} part=1 pcrc32={crc:08x}\r\n.\r\n").as_bytes(),
        );
        out
    }

    #[test]
    fn simd_break_scan_decodes_random_articles_in_random_pieces() {
        let mut x = 0x9e3779b97f4a7c15u64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for round in 0..60 {
            let size = (next() % 20_000) as usize + 1;
            // Skew some rounds toward bytes that encode to '.', '=', CR, LF.
            let data: Vec<u8> = (0..size)
                .map(|_| {
                    let r = next();
                    if round % 3 == 0 && r % 4 == 0 {
                        [b'.'.wrapping_sub(42), 0u8.wrapping_sub(42), b'\r'.wrapping_sub(42), b'='.wrapping_sub(42)][(r >> 8) as usize % 4]
                    } else {
                        r as u8
                    }
                })
                .collect();
            let art = yenc_article(&data, round % 2 == 0);
            let mut d = Decoder::new(yenc_decode_incremental, yenc_crc);
            d.crc_check = true;
            let mut got = Vec::new();
            let mut at = 0;
            while at < art.len() {
                let n = ((next() % 9000) as usize + 1).min(art.len() - at);
                let mut piece = art[at..at + n].to_vec();
                piece.resize(n + 128, 0);
                let w = d.decode_buffer(&mut piece, n);
                got.extend_from_slice(&piece[..w]);
                at += n;
            }
            assert_eq!(d.check(), Status::Finished, "round {round}");
            assert_eq!(got, data, "round {round}");
            assert_eq!(d.calculated_crc, crc32fast::hash(&data), "round {round}");
        }
    }
}
