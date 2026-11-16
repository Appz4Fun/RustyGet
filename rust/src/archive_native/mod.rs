//! Native pure-Rust RAR 1.5/2.0/2.9 (RAR3/4) and 7z (`.7z`, `.7z.001`) archive extractors.

pub mod bcj;
pub mod heap;
pub mod lzma;
pub mod ppmd_h;
pub mod rar15;
pub mod rar20;
pub mod rar3;
pub mod rarbits;

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MARK4: &[u8] = b"Rar!\x1a\x07\x00";
const SEVEN_Z: &[u8] = b"7z\xbc\xaf\x27\x1c";
const MAX_BLOCKS: usize = 65_536;
const MAX_NAME_BYTES: usize = 4096;

#[inline]
fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

#[inline]
fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

#[inline]
fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[inline]
fn le64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

#[derive(Default)]
struct Rar4KeyCache {
    entries: Vec<([u8; 8], [u8; 16], [u8; 16])>,
}

impl Rar4KeyCache {
    fn derive(&mut self, password: &str, salt: &[u8; 8]) -> ([u8; 16], [u8; 16]) {
        if let Some((_, k, i)) = self.entries.iter().find(|(s, _, _)| s == salt) {
            return (*k, *i);
        }
        let (k, i) = zipfast::crypto::derive_rar4_key_iv(password, salt);
        self.entries.push((*salt, k, i));
        (k, i)
    }
}

fn decrypt_rar4_header(
    data: &[u8],
    pos: usize,
    password: &str,
    cache: &mut Rar4KeyCache,
) -> Option<(usize, Vec<u8>, [u8; 8])> {
    let salt: [u8; 8] = data.get(pos..pos.checked_add(8)?)?.try_into().ok()?;
    let (key, iv) = cache.derive(password, &salt);
    let (header, ct_end) = zipfast::crypto::decrypt_rar4_header(data, pos + 8, &key, &iv)?;
    Some((ct_end.checked_sub(pos)?, header, salt))
}

fn encode_utf16_injective(units: &[u16]) -> String {
    let mut out = String::with_capacity(units.len());
    for res in char::decode_utf16(units.iter().copied()) {
        match res {
            Ok(ch) => {
                if ('\u{E000}'..='\u{E2FF}').contains(&ch) {
                    let cp = ch as u32;
                    out.push('\u{E200}');
                    for shift in [12u32, 8, 4, 0] {
                        out.push(char::from_u32(0xE210 + ((cp >> shift) & 0xF)).unwrap());
                    }
                } else {
                    out.push(ch);
                }
            }
            Err(unpaired) => {
                let cp = u32::from(unpaired.unpaired_surrogate());
                out.push('\u{E201}');
                for shift in [12u32, 8, 4, 0] {
                    out.push(char::from_u32(0xE210 + ((cp >> shift) & 0xF)).unwrap());
                }
            }
        }
    }
    out
}

fn unicode4(name: &[u8], enc: &[u8]) -> Option<String> {
    let mut bytes = enc.iter().copied();
    let high = bytes.next()?;
    let high = u16::from(high) << 8;
    let mut out: Vec<u16> = Vec::new();
    let (mut flags, mut bits) = (0u8, 0u32);
    while out.len() < MAX_NAME_BYTES {
        if bits == 0 {
            let Some(f) = bytes.next() else { break };
            flags = f;
            bits = 8;
        }
        let command = flags >> 6;
        flags = flags.wrapping_shl(2);
        bits -= 2;
        match command {
            0 => {
                let Some(b) = bytes.next() else { break };
                out.push(u16::from(b));
            }
            1 => {
                let Some(b) = bytes.next() else { break };
                out.push(u16::from(b) | high);
            }
            2 => {
                let (Some(a), Some(b)) = (bytes.next(), bytes.next()) else {
                    break;
                };
                out.push(u16::from_le_bytes([a, b]));
            }
            _ => {
                let Some(len) = bytes.next() else { break };
                let correction = if len & 0x80 != 0 {
                    let Some(k) = bytes.next() else { break };
                    Some(k)
                } else {
                    None
                };
                for _ in 0..usize::from(len & 0x7f) + 2 {
                    let Some(c) = name.get(out.len()).copied() else {
                        break;
                    };
                    out.push(match correction {
                        Some(k) => u16::from(c.wrapping_add(k)) | high,
                        None => u16::from(c),
                    });
                }
            }
        }
    }
    Some(encode_utf16_injective(&out))
}

fn name4(raw: &[u8], flags: u16) -> (String, Option<Vec<u8>>) {
    if flags & 0x0200 != 0 {
        if let Some(nul) = raw.iter().position(|b| *b == 0) {
            if let (Some(name), Some(enc)) = (raw.get(..nul), raw.get(nul + 1..)) {
                if let Some(decoded) = unicode4(name, enc) {
                    return (decoded, None);
                }
            }
        }
    }
    let raw_bytes = raw.split(|b| *b == 0).next().unwrap_or(raw);
    (
        encode_bytes_injective_utf8(raw_bytes),
        Some(raw_bytes.to_vec()),
    )
}

fn sanitize_rar4_rel_path_bytes(raw_name: &[u8], windows_names: bool) -> Result<PathBuf, String> {
    let unsafe_err = || format!("unsafe archive path: {}", String::from_utf8_lossy(raw_name));
    let mut out = PathBuf::new();
    // A backslash separates directories whatever the host, as unrar reads RAR 1.5-4.x names.
    let is_sep = |b: u8| b == b'/' || b == b'\\';
    for (i, part) in raw_name.split(|&b| is_sep(b)).enumerate() {
        if i == 0 && part.is_empty() && !raw_name.is_empty() {
            return Err(unsafe_err());
        }
        if part.is_empty() || part == b"." {
            continue;
        }
        if part == b".." || part.contains(&0) {
            return Err(unsafe_err());
        }
        if windows_names && i == 0 && part.len() == 2 && part[1] == b':' {
            return Err(unsafe_err());
        }
        let cleaned: Vec<u8> = if cfg!(windows) || windows_names {
            part.iter()
                .map(|&b| {
                    if matches!(b, b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*') {
                        b'_'
                    } else {
                        b
                    }
                })
                .collect()
        } else {
            part.to_vec()
        };
        #[cfg(all(unix, not(target_vendor = "apple")))]
        {
            use std::os::unix::ffi::OsStrExt;
            out.push(std::ffi::OsStr::from_bytes(&cleaned));
        }
        #[cfg(any(not(unix), target_vendor = "apple"))]
        {
            out.push(encode_bytes_injective_utf8(&cleaned));
        }
    }
    if out.as_os_str().is_empty()
        || out
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(unsafe_err());
    }
    Ok(out)
}

fn rar4_days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn rar4_dos_mtime(raw: u32) -> Option<(i64, u32)> {
    let date = raw >> 16;
    let time = raw & 0xffff;
    let year = 1980 + i64::from((date >> 9) & 0x7f);
    let month = (date >> 5) & 0x0f;
    let day = date & 0x1f;
    let hour = (time >> 11) & 0x1f;
    let minute = (time >> 5) & 0x3f;
    let second = (time & 0x1f) * 2;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = rar4_days_from_civil(year, month, day);
    Some((
        days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second),
        0,
    ))
}

fn rar4_extended_mtime(area: &[u8], dos: Option<(i64, u32)>) -> Option<(i64, u32)> {
    let flags = u16::from_le_bytes([*area.first()?, *area.get(1)?]);
    let group = (flags >> 12) & 0x0f;
    if group & 0x8 == 0 {
        return dos;
    }
    let count = usize::from(group & 0x3);
    let bytes = area.get(2..2 + count)?;
    let mut ticks = 0u32;
    for (i, &b) in bytes.iter().enumerate() {
        ticks |= u32::from(b) << (8 * (3 - count + i));
    }
    let (base_secs, _) = dos?;
    let nanos = (ticks & 0x00ff_ffff) * 100;
    Some((
        base_secs + i64::from(group & 0x4 != 0) + i64::from(nanos / 1_000_000_000),
        nanos % 1_000_000_000,
    ))
}

#[derive(Clone, Debug)]
struct Rar4FileChunk {
    name: String,
    raw_name_bytes: Option<Vec<u8>>,
    unpacked_size: u64,
    crc32: u32,
    rar_version: u8,
    method: u8,
    is_dir: bool,
    is_symlink: bool,
    unix_mode: Option<u32>,
    mtime: Option<(i64, u32)>,
    solid: bool,
    encrypted: bool,
    salt: Option<[u8; 8]>,
    split_before: bool,
    continues: bool,
    windows_names: bool,
    vol_path: PathBuf,
    data_offset: u64,
    data_len: u64,
}

struct ParsedRar4Volume {
    chunks: Vec<Rar4FileChunk>,
    has_next_volume: bool,
    may_have_next_volume: bool,
}

fn parse_rar4_volume_chunks(
    vol_path: &Path,
    password: &str,
    cache: &mut Rar4KeyCache,
) -> Result<ParsedRar4Volume, String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(vol_path)
        .map_err(|e| format!("Unrar: Could not read {}: {e}", vol_path.display()))?;
    let file_len = file
        .metadata()
        .map_err(|e| format!("Unrar: Could not read {}: {e}", vol_path.display()))?
        .len();
    if file_len < MARK4.len() as u64 {
        return Err("Not a RAR 1.5-4.x volume".to_string());
    }
    let mut sig = [0u8; 7];
    file.read_exact(&mut sig)
        .map_err(|e| format!("Unrar: Could not read {}: {e}", vol_path.display()))?;
    if sig != MARK4 {
        return Err("Not a RAR 1.5-4.x volume".to_string());
    }

    let mut chunks = Vec::new();
    let mut pos = MARK4.len() as u64;
    let mut encrypted_headers = false;
    let mut reached_end = false;
    let mut saw_end_block = false;
    let mut main_is_volume = false;
    let mut end_has_next_volume = false;
    let mut hdr_buf = vec![0u8; 65544];

    for _ in 0..MAX_BLOCKS {
        if pos >= file_len {
            reached_end = true;
            break;
        }
        let avail = (file_len - pos).min(hdr_buf.len() as u64) as usize;
        file.seek(SeekFrom::Start(pos))
            .map_err(|e| format!("Unrar: Could not seek {}: {e}", vol_path.display()))?;
        file.read_exact(&mut hdr_buf[..avail])
            .map_err(|e| format!("Unrar: Could not read {}: {e}", vol_path.display()))?;
        let rest = &hdr_buf[..avail];

        let (hdr_bytes, header, block_salt) = if encrypted_headers {
            if password.is_empty() {
                return Err("Unrar: The specified password is incorrect.".to_string());
            }
            let Some((hb, hdr, bs)) = decrypt_rar4_header(rest, 0, password, cache) else {
                return Err("Unrar: The specified password is incorrect.".to_string());
            };
            (hb, hdr, Some(bs))
        } else {
            if rest.len() < 7 {
                return Err("Unrar: Unexpected end of archive header".to_string());
            }
            let (Some(ty), Some(flags), Some(size)) =
                (rest.get(2).copied(), le16(rest, 3), le16(rest, 5))
            else {
                return Err("Unrar: Unexpected end of archive header".to_string());
            };
            let size = usize::from(size);
            if size < 7 {
                return Err("Unrar: Archive header is corrupt".to_string());
            }
            let Some(hdr) = rest.get(..size) else {
                return Err("Unrar: Unexpected end of archive header".to_string());
            };
            if matches!(ty, 0x73 | 0x74 | 0x7a | 0x7b) {
                let covered = if ty == 0x73 && flags & 0x0002 != 0 {
                    hdr.get(2..13)
                } else {
                    hdr.get(2..)
                };
                let stored = le16(hdr, 0);
                if covered.is_none_or(|c| Some(crc32(c) as u16) != stored) {
                    return Err("Unrar: Archive header checksum error".to_string());
                }
            }
            (size, hdr.to_vec(), None)
        };

        let (Some(ty), Some(flags)) = (header.get(2).copied(), le16(&header, 3)) else {
            return Err("Unrar: Corrupt block header".to_string());
        };
        if ty == 0x7b {
            saw_end_block = true;
            end_has_next_volume = (flags & 0x0001) != 0;
            reached_end = true;
            break;
        }
        if ty == 0x73 {
            main_is_volume = (flags & 0x0001) != 0;
            if !encrypted_headers && (flags & 0x0080 != 0) {
                encrypted_headers = true;
                pos = pos.saturating_add(hdr_bytes as u64);
                continue;
            }
        }

        let mut add = 0u64;
        if flags & 0x8000 != 0 {
            let Some(low) = le32(&header, 7) else {
                return Err("Unrar: Corrupt block header".to_string());
            };
            add = u64::from(low);
        }

        let data_at = pos.saturating_add(hdr_bytes as u64);
        if ty == 0x74 {
            if header.len() < 32 {
                return Err("Unrar: Corrupt file header".to_string());
            }
            let unp_low = u64::from(le32(&header, 11).unwrap_or(0));
            let host_os = header.get(15).copied().unwrap_or(0);
            let file_crc = le32(&header, 16).unwrap_or(0);
            let dos_time = le32(&header, 20).unwrap_or(0);
            let rar_version = header.get(24).copied().unwrap_or(29);
            let method = header.get(25).copied().unwrap_or(0x30);
            let name_len = usize::from(le16(&header, 26).unwrap_or(0));
            let attr = le32(&header, 28).unwrap_or(0);
            let (high_pack, high_unp, name_at) = if flags & 0x0100 != 0 {
                if header.len() < 40 {
                    return Err("Unrar: Corrupt 64-bit file header".to_string());
                }
                (
                    u64::from(le32(&header, 32).unwrap_or(0)),
                    u64::from(le32(&header, 36).unwrap_or(0)),
                    40usize,
                )
            } else {
                (0u64, 0u64, 32usize)
            };
            add |= high_pack << 32;
            let unpacked_size = unp_low | (high_unp << 32);
            let name_end = (name_at + name_len).min(header.len());
            let raw_name = header.get(name_at..name_end).unwrap_or_default();
            let (name, raw_name_bytes) = name4(raw_name, flags);
            let mut ext_cursor = name_end;
            let mut salt = if flags & 0x0400 != 0 {
                let s = header
                    .get(ext_cursor..ext_cursor + 8)
                    .and_then(|s| s.try_into().ok());
                ext_cursor = (ext_cursor + 8).min(header.len());
                s
            } else {
                None
            };
            if salt.is_none() {
                salt = block_salt;
            }
            let mut mtime = rar4_dos_mtime(dos_time);
            if flags & 0x1000 != 0 {
                mtime = rar4_extended_mtime(&header[ext_cursor..], mtime).or(mtime);
            }
            let data_end = data_at
                .checked_add(add)
                .ok_or_else(|| "Unrar: Packed data offset overflow".to_string())?;
            if data_end > file_len {
                return Err("Unrar: Unexpected end of archive data".to_string());
            }
            let is_dir = (flags & 0x00e0) == 0x00e0;
            let unix_mode_raw = if host_os == 3 {
                if (attr & 0xffff) != 0 {
                    attr & 0xffff
                } else {
                    attr >> 16
                }
            } else {
                0
            };
            let is_symlink = host_os == 3 && (unix_mode_raw & 0o170000) == 0o120000;
            let unix_mode = (host_os == 3 && unix_mode_raw != 0).then_some(unix_mode_raw);
            chunks.push(Rar4FileChunk {
                name,
                raw_name_bytes,
                unpacked_size,
                crc32: file_crc,
                rar_version,
                method,
                is_dir,
                is_symlink,
                unix_mode,
                mtime,
                solid: (flags & 0x0010) != 0,
                encrypted: (flags & 0x0004 != 0) || encrypted_headers,
                salt,
                split_before: (flags & 0x0001) != 0,
                continues: (flags & 0x0002) != 0,
                windows_names: host_os != 3,
                vol_path: vol_path.to_path_buf(),
                data_offset: data_at,
                data_len: add,
            });
        }

        let next = data_at
            .checked_add(add)
            .ok_or_else(|| "Unrar: Packed data offset overflow".to_string())?;
        if next <= pos || next > file_len {
            return Err("Unrar: Unexpected end of archive data".to_string());
        }
        pos = next;
    }

    if !reached_end {
        return Err("Unrar: Archive exceeds maximum block count".to_string());
    }

    let has_next_volume = end_has_next_volume || chunks.last().is_some_and(|c| c.continues);
    let may_have_next_volume = has_next_volume || (!saw_end_block && main_is_volume);
    Ok(ParsedRar4Volume {
        chunks,
        has_next_volume,
        may_have_next_volume,
    })
}

struct Rar4ChunkStreamReader<'a> {
    chunks: &'a [Rar4FileChunk],
    chunk_idx: usize,
    chunk_rem: u64,
    cur_file: Option<std::fs::File>,
    encrypted: bool,
    per_vol: bool,
    password: &'a str,
    first_salt: Option<[u8; 8]>,
    cur_key: [u8; 16],
    cur_iv: [u8; 16],
    have_key: bool,
    raw_buf: Vec<u8>,
}

impl<'a> Rar4ChunkStreamReader<'a> {
    fn new(chunks: &'a [Rar4FileChunk], encrypted: bool, per_vol: bool, password: &'a str) -> Self {
        let first_salt = chunks.first().and_then(|c| c.salt);
        Self {
            chunks,
            chunk_idx: 0,
            chunk_rem: 0,
            cur_file: None,
            encrypted,
            per_vol,
            password,
            first_salt,
            cur_key: [0u8; 16],
            cur_iv: [0u8; 16],
            have_key: false,
            raw_buf: Vec::with_capacity(256 * 1024),
        }
    }

    fn advance_chunk(&mut self, cache: &mut Rar4KeyCache) -> Result<bool, String> {
        use std::io::{Seek, SeekFrom};
        while self.chunk_rem == 0 {
            if self.chunk_idx >= self.chunks.len() {
                self.cur_file = None;
                return Ok(false);
            }
            let ch = &self.chunks[self.chunk_idx];
            self.chunk_idx += 1;
            let mut f = std::fs::File::open(&ch.vol_path)
                .map_err(|e| format!("Unrar: Could not read {}: {e}", ch.vol_path.display()))?;
            f.seek(SeekFrom::Start(ch.data_offset))
                .map_err(|e| format!("Unrar: Could not seek {}: {e}", ch.vol_path.display()))?;
            self.chunk_rem = ch.data_len;
            self.cur_file = Some(f);
            if self.encrypted && (self.per_vol || !self.have_key) {
                let Some(salt) = ch.salt.or(self.first_salt) else {
                    return Err("Unrar: Missing encryption salt".to_string());
                };
                let (k, iv) = cache.derive(self.password, &salt);
                self.cur_key = k;
                self.cur_iv = iv;
                self.have_key = true;
            }
        }
        Ok(true)
    }

    fn read_next_block(
        &mut self,
        out: &mut Vec<u8>,
        cache: &mut Rar4KeyCache,
    ) -> Result<usize, String> {
        use std::io::Read;
        const BLOCK_BYTES: usize = 256 * 1024;

        self.raw_buf.clear();
        while self.raw_buf.len() < BLOCK_BYTES {
            if !self.advance_chunk(cache)? {
                break;
            }
            let want = (BLOCK_BYTES - self.raw_buf.len()).min(self.chunk_rem as usize);
            let cur_len = self.raw_buf.len();
            self.raw_buf.resize(cur_len + want, 0);
            let f = self.cur_file.as_mut().unwrap();
            f.read_exact(&mut self.raw_buf[cur_len..cur_len + want])
                .map_err(|e| format!("Unrar: Read error: {e}"))?;
            self.chunk_rem -= want as u64;
            if self.per_vol && self.chunk_rem == 0 {
                break;
            }
        }

        if self.raw_buf.is_empty() {
            return Ok(0);
        }

        if !self.encrypted {
            out.extend_from_slice(&self.raw_buf);
            return Ok(self.raw_buf.len());
        }

        if self.raw_buf.len() % 16 != 0 {
            return Err("Unrar: Encrypted stream not aligned to AES block size".to_string());
        }
        let next_iv: [u8; 16] = self.raw_buf[self.raw_buf.len() - 16..].try_into().unwrap();
        let Some(dec) = zipfast::crypto::decrypt_cbc(&self.cur_key, &self.cur_iv, &self.raw_buf)
        else {
            return Err("Unrar: Failed to decrypt block".to_string());
        };
        self.cur_iv = next_iv;
        let n = dec.len();
        out.extend_from_slice(&dec);
        Ok(n)
    }
}

#[derive(Default, Clone)]
struct RarDecoderSuite {
    r15: rar15::Rar15Decoder,
    r20: rar20::Rar20Decoder,
    r29: rar3::Rar29Decoder,
}

impl RarDecoderSuite {
    fn decode_stream(
        &mut self,
        reader: &mut Rar4ChunkStreamReader<'_>,
        cache: &mut Rar4KeyCache,
        method: u8,
        rar_version: u8,
        unpacked_size: u64,
        solid: bool,
        on_output: &mut dyn FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        if method == 0x30 {
            let mut rem = unpacked_size;
            let mut buf = Vec::with_capacity(256 * 1024);
            while rem > 0 {
                buf.clear();
                let n = reader.read_next_block(&mut buf, cache)?;
                if n == 0 {
                    return Err("Unrar: Stored stream shorter than unpacked size".to_string());
                }
                let take_n = buf.len().min(usize::try_from(rem).unwrap_or(usize::MAX));
                on_output(&buf[..take_n])?;
                rem -= take_n as u64;
            }
            return Ok(());
        }
        let target = usize::try_from(unpacked_size).map_err(|_| "Unpacked size overflow")?;
        match rar_version {
            15 | 20 | 26 => {
                let mut packed = Vec::new();
                while reader.read_next_block(&mut packed, cache)? > 0 {}
                let out = if rar_version == 15 {
                    self.r15.decode(&packed, target, solid)?
                } else {
                    self.r20.decode(&packed, target, solid)?
                };
                on_output(&out)
            }
            _ => self.r29.decode_stream(
                target,
                solid,
                &mut |in_buf| reader.read_next_block(in_buf, cache),
                on_output,
            ),
        }
    }
}

fn encode_bytes_injective_utf8(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        for ch in chunk.valid().chars() {
            if ('\u{E000}'..='\u{E1FF}').contains(&ch) {
                let mut u8_buf = [0u8; 4];
                out.push('\u{E100}');
                for &b in ch.encode_utf8(&mut u8_buf).as_bytes() {
                    out.push(char::from_u32(0xE000 + u32::from(b)).unwrap());
                }
            } else {
                out.push(ch);
            }
        }
        for &b in chunk.invalid() {
            out.push(char::from_u32(0xE000 + u32::from(b)).unwrap());
        }
    }
    out
}

#[cfg(unix)]
fn is_invalid_utf8_fs_error(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(libc::EILSEQ)
}

#[cfg(not(unix))]
fn is_invalid_utf8_fs_error(_err: &std::io::Error) -> bool {
    false
}

#[cfg(unix)]
fn encode_os_str_injective_utf8(part: &std::ffi::OsStr) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsString::from(encode_bytes_injective_utf8(part.as_bytes()))
}

#[cfg(not(unix))]
fn encode_os_str_injective_utf8(part: &std::ffi::OsStr) -> std::ffi::OsString {
    part.to_os_string()
}

pub(crate) fn create_dir_all_no_follow(
    output_dir: &Path,
    sanitized_rel: &Path,
) -> Result<PathBuf, String> {
    if let Ok(meta) = std::fs::symlink_metadata(output_dir) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "Refusing to extract into symlinked output root {}",
                output_dir.display()
            ));
        }
    } else {
        std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    }

    let mut cur = output_dir.to_path_buf();
    for comp in sanitized_rel.components() {
        let std::path::Component::Normal(part) = comp else {
            return Err(format!(
                "Invalid relative path component in {}",
                sanitized_rel.display()
            ));
        };
        cur.push(part);
        // The job's unpack limits (`unpacklimits`), before anything is looked up or created.
        crate::unpacklimits::probe(&cur, true)?;
        let meta_res = match std::fs::symlink_metadata(&cur) {
            Err(e) if is_invalid_utf8_fs_error(&e) => {
                cur.pop();
                cur.push(encode_os_str_injective_utf8(part));
                std::fs::symlink_metadata(&cur)
            }
            res => res,
        };
        crate::unpacklimits::check(&cur, true)?;
        match meta_res {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(format!("Refusing to follow symlink at {}", cur.display()));
                }
                if !meta.is_dir() {
                    return Err(format!(
                        "Expected directory at {}, found non-directory",
                        cur.display()
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Err(create_err) = std::fs::create_dir(&cur) {
                    if is_invalid_utf8_fs_error(&create_err) {
                        cur.pop();
                        cur.push(encode_os_str_injective_utf8(part));
                        std::fs::create_dir(&cur).map_err(|e| e.to_string())?;
                    } else {
                        return Err(create_err.to_string());
                    }
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(cur)
}

#[cfg(unix)]
fn open_file_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_file_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

pub(crate) fn create_dest_file_no_follow(
    output_dir: &Path,
    sanitized_rel: &Path,
    overwrite_mode: i32,
) -> Result<Option<std::fs::File>, String> {
    Ok(
        create_dest_file_with_path_no_follow(output_dir, sanitized_rel, overwrite_mode)?
            .map(|(f, _, _)| f),
    )
}

fn create_dest_file_with_path_no_follow(
    output_dir: &Path,
    sanitized_rel: &Path,
    overwrite_mode: i32,
) -> Result<Option<(std::fs::File, PathBuf, bool)>, String> {
    let parent_rel = sanitized_rel.parent().unwrap_or_else(|| Path::new(""));
    let file_name = sanitized_rel
        .file_name()
        .ok_or_else(|| "Destination path is missing file name".to_string())?;
    let parent_dir = create_dir_all_no_follow(output_dir, parent_rel)?;
    let mut candidate = parent_dir.join(file_name);
    crate::unpacklimits::probe(&candidate, false)?;
    let meta_res = match std::fs::symlink_metadata(&candidate) {
        Err(e) if is_invalid_utf8_fs_error(&e) => {
            candidate = parent_dir.join(encode_os_str_injective_utf8(file_name));
            std::fs::symlink_metadata(&candidate)
        }
        res => res,
    };

    let (target_path, created_new) = match meta_res {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "Refusing to overwrite symlink at {}",
                    candidate.display()
                ));
            }
            match overwrite_mode {
                0 => return Ok(None), // Skip
                2 => {
                    // AutoRename
                    let stem = candidate
                        .file_stem()
                        .map(|s| s.to_os_string())
                        .unwrap_or_else(|| std::ffi::OsString::from("file"));
                    let ext = candidate.extension().map(|s| s.to_os_string());
                    let mut chosen = None;
                    for n in 1..10_000u32 {
                        let mut new_name = stem.clone();
                        new_name.push(format!("_{n}"));
                        if let Some(ref e) = ext {
                            new_name.push(".");
                            new_name.push(e);
                        }
                        let alt = parent_dir.join(new_name);
                        match std::fs::symlink_metadata(&alt) {
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                chosen = Some(alt);
                                break;
                            }
                            Ok(alt_meta) if alt_meta.file_type().is_symlink() => {
                                continue;
                            }
                            _ => continue,
                        }
                    }
                    match chosen {
                        Some(p) => (p, true),
                        None => (candidate, false),
                    }
                }
                _ => (candidate, false), // Overwrite
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (candidate, true),
        Err(e) => return Err(e.to_string()),
    };

    if let Ok(meta) = std::fs::symlink_metadata(&target_path) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "Refusing to overwrite symlink at {}",
                target_path.display()
            ));
        }
    }

    // The name finally created (after an automatic rename, or an injective encoding of a
    // non-UTF-8 name) is the one counted.
    crate::unpacklimits::check(&target_path, false)?;
    let file = open_file_no_follow(&target_path).map_err(|e| e.to_string())?;
    Ok(Some((file, target_path, created_new)))
}

#[cfg(unix)]
fn apply_unix_executable_mode(file: &std::fs::File, mode: u32) {
    if (mode & 0o111) != 0 {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode((mode & 0o777) | 0o600));
    }
}

#[cfg(not(unix))]
fn apply_unix_executable_mode(_file: &std::fs::File, _mode: u32) {}

#[allow(clippy::too_many_arguments)]
fn process_ready_rar4_chunks<F>(
    pending: &mut std::collections::VecDeque<Rar4FileChunk>,
    flush_incomplete: bool,
    output_dir: &Path,
    password: &str,
    overwrite_mode: i32,
    cache: &mut Rar4KeyCache,
    decoders: &mut RarDecoderSuite,
    summary: &mut zipfast::extract::Summary,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), String>
where
    F: FnMut(i32, &str),
{
    use std::io::{Seek, SeekFrom};

    while let Some(first) = pending.front() {
        if is_cancelled() {
            return Err("Unpack cancelled".to_string());
        }
        if first.continues && !flush_incomplete {
            // Check whether all continuation chunks for this member have arrived
            let mut end_idx = 0usize;
            let mut complete = false;
            while let Some(ch) = pending.get(end_idx) {
                if !ch.continues {
                    complete = true;
                    break;
                }
                end_idx += 1;
            }
            if !complete {
                break;
            }
        }

        let first = pending.pop_front().unwrap();
        if first.is_symlink {
            log_cb(1, "ERROR: Dangerous link path was ignored");
            log_cb(0, "Stream repair: archive contains link");
            return Err("archive contains link".to_string());
        }
        let rel = match first.raw_name_bytes {
            Some(ref raw_bytes) => sanitize_rar4_rel_path_bytes(raw_bytes, first.windows_names)
                .map_err(|e| {
                    let msg = format!("Unrar: {e}");
                    log_cb(1, &msg);
                    msg
                })?,
            // A backslash separates directories whatever the host, as for raw names.
            None => zipfast::path::sanitize(&first.name.replace('\\', "/"), first.windows_names)
                .map_err(|e| {
                    let msg = format!("Unrar: {e}");
                    log_cb(1, &msg);
                    msg
                })?,
        };
        if first.is_dir {
            create_dir_all_no_follow(output_dir, &rel).map_err(|e| {
                let msg = format!("Unrar: {e}");
                log_cb(1, &msg);
                msg
            })?;
            summary.dirs += 1;
            continue;
        }
        if first.split_before {
            let msg =
                "Unrar: WARNING: You need to start extraction from a previous volume".to_string();
            log_cb(1, &msg);
            return Err(msg);
        }
        if first.encrypted && password.is_empty() {
            let msg = "Unrar: The specified password is incorrect.".to_string();
            log_cb(1, &msg);
            return Err(msg);
        }

        let mut member_chunks = vec![first.clone()];
        while member_chunks.last().unwrap().continues {
            let Some(next_chunk) = pending.pop_front() else {
                let msg = format!("Unrar: {} : packed data CRC failed in volume", first.name);
                log_cb(1, &msg);
                return Err(msg);
            };
            if !next_chunk.split_before {
                let msg = format!("Unrar: {} : packed data CRC failed in volume", first.name);
                log_cb(1, &msg);
                return Err(msg);
            }
            member_chunks.push(next_chunk);
        }
        let last_chunk = member_chunks.last().unwrap().clone();

        log_cb(4, &format!("Unrar: Extracting  {}", first.name));
        if is_cancelled() {
            return Err("Unpack cancelled".to_string());
        }

        let mut dest = create_dest_file_with_path_no_follow(output_dir, &rel, overwrite_mode)
            .map_err(|e| {
                let msg = format!("Unrar: {e}");
                log_cb(1, &msg);
                msg
            })?;

        let cleanup_dest = |dest: Option<(std::fs::File, PathBuf, bool)>| {
            if let Some((f, path, created_new)) = dest {
                drop(f);
                if created_new {
                    let _ = std::fs::remove_file(path);
                }
            }
        };

        if first.encrypted {
            let saved_decoders = decoders.clone();
            let mut matched = false;
            let modes: &[bool] = if member_chunks.len() > 1 {
                &[false, true]
            } else {
                &[false]
            };
            for (attempt, &per_vol) in modes.iter().enumerate() {
                if is_cancelled() {
                    cleanup_dest(dest);
                    return Err("Unpack cancelled".to_string());
                }
                if attempt > 0 {
                    *decoders = saved_decoders.clone();
                    if let Some((f, _, _)) = dest.as_mut() {
                        let _ = f.set_len(0);
                        let _ = f.seek(SeekFrom::Start(0));
                    }
                }
                let mut reader =
                    Rar4ChunkStreamReader::new(&member_chunks, true, per_vol, password);
                let mut hasher = crc32fast::Hasher::new();
                let mut written = 0u64;
                let mut cancelled = false;
                let res = decoders.decode_stream(
                    &mut reader,
                    cache,
                    first.method,
                    first.rar_version,
                    last_chunk.unpacked_size,
                    first.solid,
                    &mut |slice| {
                        if is_cancelled() {
                            cancelled = true;
                            return Err("Unpack cancelled".to_string());
                        }
                        hasher.update(slice);
                        written += slice.len() as u64;
                        if let Some((f, _, _)) = dest.as_mut() {
                            f.write_all(slice).map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    },
                );
                if cancelled {
                    cleanup_dest(dest);
                    return Err("Unpack cancelled".to_string());
                }
                if res.is_ok()
                    && written == last_chunk.unpacked_size
                    && hasher.finalize() == last_chunk.crc32
                {
                    matched = true;
                    break;
                }
            }
            if !matched {
                cleanup_dest(dest);
                let msg = format!("Unrar: Checksum error in the encrypted file {}", first.name);
                log_cb(1, &msg);
                let pw_msg = "Unrar: The specified password is incorrect.".to_string();
                log_cb(1, &pw_msg);
                return Err(pw_msg);
            }
        } else {
            let mut reader = Rar4ChunkStreamReader::new(&member_chunks, false, false, password);
            let mut hasher = crc32fast::Hasher::new();
            let mut written = 0u64;
            let mut cancelled = false;
            if let Err(e) = decoders.decode_stream(
                &mut reader,
                cache,
                first.method,
                first.rar_version,
                last_chunk.unpacked_size,
                first.solid,
                &mut |slice| {
                    if is_cancelled() {
                        cancelled = true;
                        return Err("Unpack cancelled".to_string());
                    }
                    hasher.update(slice);
                    written += slice.len() as u64;
                    if let Some((f, _, _)) = dest.as_mut() {
                        f.write_all(slice).map_err(|e| e.to_string())?;
                    }
                    Ok(())
                },
            ) {
                cleanup_dest(dest);
                if cancelled {
                    return Err("Unpack cancelled".to_string());
                }
                let crc_msg = format!("Unrar: {} - CRC failed", first.name);
                log_cb(1, &crc_msg);
                log_cb(1, &e);
                return Err(crc_msg);
            }
            if written != last_chunk.unpacked_size || hasher.finalize() != last_chunk.crc32 {
                cleanup_dest(dest);
                let crc_msg = format!("Unrar: {} - CRC failed", first.name);
                log_cb(1, &crc_msg);
                return Err(crc_msg);
            }
        }

        match dest {
            Some((f, _, _)) => {
                if let Some(mode) = first.unix_mode {
                    apply_unix_executable_mode(&f, mode);
                }
                if let Some((secs, nanos)) = first.mtime.or(last_chunk.mtime) {
                    set_file_mtime(&f, Some(secs), nanos);
                }
                summary.files += 1;
                summary.bytes += last_chunk.unpacked_size;
            }
            None => {
                summary.skipped += 1;
            }
        }
    }
    Ok(())
}

/// Extracts a RAR 1.5–4.x single or multi-volume archive into `output_dir`.
/// A following volume can be read: it exists and is not a damaged file that direct unpack
/// keeps on hold until its repair finished.
fn volume_ready(path: &std::path::Path) -> bool {
    if !path.exists() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        !crate::postproc::direct_unpack_is_held_path(path.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn extract_rar4<F>(
    archive_path: &Path,
    output_dir: &Path,
    password: &str,
    overwrite_mode: i32,
    log_cb: &mut F,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    extract_rar4_cancellable(
        archive_path,
        output_dir,
        password,
        overwrite_mode,
        log_cb,
        &mut || false,
    )
}

/// Extracts a RAR 1.5–4.x single or multi-volume archive into `output_dir`, polling `is_cancelled`.
pub fn extract_rar4_cancellable<F>(
    archive_path: &Path,
    output_dir: &Path,
    password: &str,
    overwrite_mode: i32,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    log_cb(
        4,
        "Unrar: UNRAR 7.00 (RustyGet built-in) Copyright (c) 1993-2026 Alexander Roshal",
    );

    let mut cache = Rar4KeyCache::default();
    let mut decoders = RarDecoderSuite::default();
    let mut pending_chunks: std::collections::VecDeque<Rar4FileChunk> =
        std::collections::VecDeque::new();
    let mut summary = zipfast::extract::Summary::default();
    let mut cur_vol = archive_path.to_path_buf();
    let mut reached_final_volume = false;

    for _ in 0..4096 {
        if is_cancelled() {
            return Err("Unpack cancelled".to_string());
        }
        let vol_name = cur_vol
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("archive.rar");
        log_cb(4, &format!("Unrar: Extracting from {vol_name}"));
        let parsed = parse_rar4_volume_chunks(&cur_vol, password, &mut cache).map_err(|e| {
            log_cb(1, &e);
            e
        })?;
        summary.volumes += 1;
        for c in parsed.chunks {
            pending_chunks.push_back(c);
        }
        process_ready_rar4_chunks(
            &mut pending_chunks,
            false,
            output_dir,
            password,
            overwrite_mode,
            &mut cache,
            &mut decoders,
            &mut summary,
            log_cb,
            is_cancelled,
        )?;

        if !parsed.may_have_next_volume {
            reached_final_volume = true;
            break;
        }
        let Some(next_vol) = zipfast::volume::next_volume(&cur_vol) else {
            if parsed.has_next_volume {
                let msg = "Unrar: Cannot find next volume".to_string();
                log_cb(1, &msg);
                return Err(msg);
            }
            reached_final_volume = true;
            break;
        };
        if !volume_ready(&next_vol) {
            if parsed.has_next_volume || next_vol.exists() {
                let next_name = next_vol
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("next volume");
                log_cb(
                    4,
                    &format!("Unrar: Insert disk with {next_name} [C]ontinue, [Q]uit"),
                );
                if !volume_ready(&next_vol) {
                    let msg = format!("Unrar: Cannot find volume {next_name}");
                    log_cb(1, &msg);
                    return Err(msg);
                }
            } else {
                reached_final_volume = true;
                break;
            }
        }
        cur_vol = next_vol;
    }

    if !reached_final_volume {
        let msg = "Unrar: Archive exceeds maximum volume count".to_string();
        log_cb(1, &msg);
        return Err(msg);
    }

    process_ready_rar4_chunks(
        &mut pending_chunks,
        true,
        output_dir,
        password,
        overwrite_mode,
        &mut cache,
        &mut decoders,
        &mut summary,
        log_cb,
        is_cancelled,
    )?;

    log_cb(4, "Unrar: All OK");
    Ok(summary)
}

// ============================================================================
// Pure-Rust 7z (`.7z` and `.7z.001`) Archive Extractor
// ============================================================================

const SZ_MAX_DECODED: u64 = 16 * 1024 * 1024 * 1024;
const SZ_MAX_FOLDERS: u64 = 4_096;
const SZ_MAX_PACK_STREAMS: u64 = 4_096;
const SZ_MAX_CODERS: u64 = 32;
const SZ_MAX_CODER_PROPS: u64 = 64;
const SZ_MAX_ENTRIES: usize = 65_536;

fn sz_number(b: &[u8], pos: &mut usize) -> Option<u64> {
    let first = *b.get(*pos)?;
    *pos += 1;
    let extra = first.leading_ones();
    let mut value = 0u64;
    for i in 0..extra {
        value |= u64::from(*b.get(*pos)?) << (8 * i);
        *pos += 1;
    }
    if extra < 8 {
        value |= u64::from(first & (0x7fu8 >> extra)) << (8 * extra);
    }
    Some(value)
}

struct SzReader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> SzReader<'a> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.b.get(self.p)?;
        self.p += 1;
        Some(b)
    }

    fn number(&mut self) -> Option<u64> {
        sz_number(self.b, &mut self.p)
    }

    fn take(&mut self, n: u64) -> Option<&'a [u8]> {
        let n = usize::try_from(n).ok()?;
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }

    fn left(&self) -> u64 {
        self.b.len().saturating_sub(self.p) as u64
    }

    fn bits(&mut self, count: u64) -> Option<Vec<bool>> {
        if count.div_ceil(8) > self.left() {
            return None;
        }
        let bytes = self.take(count.div_ceil(8))?;
        let mut out = Vec::with_capacity(usize::try_from(count).ok()?);
        for i in 0..usize::try_from(count).ok()? {
            let byte = *bytes.get(i / 8)?;
            out.push(byte & (0x80 >> (i % 8)) != 0);
        }
        Some(out)
    }

    fn digests(&mut self, count: u64) -> Option<Vec<Option<u32>>> {
        if count > self.left().saturating_mul(8) {
            return None;
        }
        let defined = if self.byte()? == 0 {
            self.bits(count)?
        } else {
            vec![true; usize::try_from(count).ok()?]
        };
        defined
            .into_iter()
            .map(|d| {
                if d {
                    let crc = u32::from_le_bytes(self.take(4)?.try_into().ok()?);
                    Some(Some(crc))
                } else {
                    Some(None)
                }
            })
            .collect()
    }
}

#[derive(Default, Clone, Debug)]
struct SzCoder {
    id: Vec<u8>,
    props: Vec<u8>,
    num_in: usize,
    num_out: usize,
}

#[derive(Default, Clone, Debug)]
struct SzFolder {
    coders: Vec<SzCoder>,
    bind_pairs: Vec<(u64, u64)>,
    packed_streams: Vec<u64>,
    out_streams: u64,
    unpack: Vec<u64>,
    crc: Option<u32>,
}

impl SzFolder {
    fn final_out_stream_index(&self) -> Option<usize> {
        (0..self.out_streams)
            .find(|i| !self.bind_pairs.iter().any(|(_, out_idx)| out_idx == i))
            .and_then(|i| usize::try_from(i).ok())
    }

    fn size(&self) -> Option<u64> {
        let idx = self.final_out_stream_index()?;
        self.unpack.get(idx).copied()
    }
}

#[derive(Default, Clone, Debug)]
struct SzStreams {
    pack_pos: u64,
    pack_sizes: Vec<u64>,
    folders: Vec<SzFolder>,
    per_folder: Vec<u64>,
    sizes: Vec<u64>,
    digests: Vec<Option<u32>>,
}

fn sz_folder(r: &mut SzReader<'_>) -> Option<SzFolder> {
    let mut f = SzFolder::default();
    let coders = r.number()?;
    if coders == 0 || coders > SZ_MAX_CODERS {
        return None;
    }
    let (mut in_total, mut out_total) = (0u64, 0u64);
    for _ in 0..coders {
        let flags = r.byte()?;
        if flags & 0xc0 != 0 {
            return None;
        }
        let id = r.take(u64::from(flags & 0x0f))?.to_vec();
        let (mut ins, mut outs) = (1u64, 1u64);
        if flags & 0x10 != 0 {
            ins = r.number()?;
            outs = r.number()?;
            if ins == 0 || outs == 0 || ins > SZ_MAX_CODERS || outs > SZ_MAX_CODERS {
                return None;
            }
        }
        let props = if flags & 0x20 != 0 {
            let size = r.number()?;
            if size > SZ_MAX_CODER_PROPS {
                return None;
            }
            r.take(size)?.to_vec()
        } else {
            Vec::new()
        };
        f.coders.push(SzCoder {
            id,
            props,
            num_in: ins as usize,
            num_out: outs as usize,
        });
        in_total += ins;
        out_total += outs;
    }
    if in_total > SZ_MAX_CODERS || out_total > SZ_MAX_CODERS {
        return None;
    }
    let pairs = out_total.checked_sub(1)?;
    let packed = in_total.checked_sub(pairs)?;
    for _ in 0..pairs {
        let in_idx = r.number()?;
        let out_idx = r.number()?;
        if in_idx >= in_total
            || out_idx >= out_total
            || f.bind_pairs
                .iter()
                .any(|&(bi, bo)| bi == in_idx || bo == out_idx)
        {
            return None;
        }
        f.bind_pairs.push((in_idx, out_idx));
    }
    if packed > 1 {
        for _ in 0..packed {
            let p_in = r.number()?;
            if p_in >= in_total
                || f.bind_pairs.iter().any(|&(bi, _)| bi == p_in)
                || f.packed_streams.contains(&p_in)
            {
                return None;
            }
            f.packed_streams.push(p_in);
        }
    } else if packed == 1 {
        let unbound_in =
            (0..in_total).find(|i| !f.bind_pairs.iter().any(|(in_idx, _)| in_idx == i))?;
        f.packed_streams.push(unbound_in);
    }
    f.out_streams = out_total;
    Some(f)
}

fn sz_streams(r: &mut SzReader<'_>) -> Option<SzStreams> {
    let mut s = SzStreams::default();
    let mut id = r.byte()?;
    if id == 0x06 {
        s.pack_pos = r.number()?;
        let count = r.number()?;
        if count > r.left() || count > SZ_MAX_PACK_STREAMS {
            return None;
        }
        loop {
            match r.byte()? {
                0x00 => break,
                0x09 => {
                    for _ in 0..count {
                        s.pack_sizes.push(r.number()?);
                    }
                }
                0x0a => {
                    r.digests(count)?;
                }
                _ => return None,
            }
        }
        id = r.byte()?;
    }
    if id == 0x07 {
        if r.byte()? != 0x0b {
            return None;
        }
        let count = r.number()?;
        if count > SZ_MAX_FOLDERS || r.byte()? != 0 {
            return None;
        }
        for _ in 0..count {
            s.folders.push(sz_folder(r)?);
        }
        if r.byte()? != 0x0c {
            return None;
        }
        for f in &mut s.folders {
            for _ in 0..f.out_streams {
                f.unpack.push(r.number()?);
            }
        }
        loop {
            match r.byte()? {
                0x00 => break,
                0x0a => {
                    let digests = r.digests(count)?;
                    for (f, d) in s.folders.iter_mut().zip(digests) {
                        f.crc = d;
                    }
                }
                _ => return None,
            }
        }
        id = r.byte()?;
    }
    let mut per_folder: Vec<u64> = vec![1; s.folders.len()];
    let mut listed = false;
    let mut sub_digests: Vec<Option<u32>> = Vec::new();
    if id == 0x08 {
        id = r.byte()?;
        if id == 0x0d {
            for n in &mut per_folder {
                *n = r.number()?;
            }
            id = r.byte()?;
        }
        let cap = 4 * SZ_MAX_ENTRIES as u64;
        if per_folder.iter().try_fold(0u64, |t, n| t.checked_add(*n))? > cap {
            return None;
        }
        if id == 0x09 {
            listed = true;
            for (f, n) in s.folders.iter().zip(&per_folder) {
                if *n == 0 {
                    continue;
                }
                let mut left = f.size()?;
                for _ in 1..*n {
                    let size = r.number()?;
                    left = left.checked_sub(size)?;
                    s.sizes.push(size);
                }
                s.sizes.push(left);
            }
            id = r.byte()?;
        }
        while id != 0x00 {
            if id != 0x0a {
                return None;
            }
            let count: u64 = s
                .folders
                .iter()
                .zip(&per_folder)
                .map(|(f, n)| if *n == 1 && f.crc.is_some() { 0 } else { *n })
                .sum();
            sub_digests = r.digests(count)?;
            id = r.byte()?;
        }
        id = r.byte()?;
    }
    if !listed {
        for (f, n) in s.folders.iter().zip(&per_folder) {
            if *n == 1 {
                s.sizes.push(f.size()?);
            }
        }
    }
    let mut dig_iter = sub_digests.into_iter();
    for (f, &n) in s.folders.iter().zip(&per_folder) {
        if n == 1 && f.crc.is_some() {
            s.digests.push(f.crc);
        } else {
            for _ in 0..n {
                s.digests.push(dig_iter.next().flatten());
            }
        }
    }
    s.per_folder = per_folder;
    (id == 0x00).then_some(s)
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub(crate) struct Sha256State {
    h: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Sha256State {
    pub(crate) fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn compress_block(h: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    pub(crate) fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                Self::compress_block(&mut self.h, &self.buf);
                self.buf_len = 0;
            }
        }
        let mut iter = data.chunks_exact(64);
        for chunk in &mut iter {
            let block: &[u8; 64] = chunk.try_into().unwrap();
            Self::compress_block(&mut self.h, block);
        }
        let rem = iter.remainder();
        if !rem.is_empty() {
            self.buf[..rem.len()].copy_from_slice(rem);
            self.buf_len = rem.len();
        }
    }

    pub(crate) fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            self.buf[self.buf_len..64].fill(0);
            Self::compress_block(&mut self.h, &self.buf);
            self.buf_len = 0;
        }
        self.buf[self.buf_len..56].fill(0);
        self.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
        Self::compress_block(&mut self.h, &self.buf);
        let mut out = [0u8; 32];
        for (i, &word) in self.h.iter().enumerate() {
            out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

fn sz_derive_aes256_key_iv(props: &[u8], password: &str) -> Result<([u8; 32], [u8; 16]), String> {
    if password.is_empty() {
        return Err("Cannot open encrypted archive. Wrong password?".to_string());
    }
    let first = *props
        .first()
        .ok_or_else(|| "Cannot open encrypted archive. Wrong password?".to_string())?;
    let num_cycles_power = first & 0x3f;
    let salt_size = if first & 0x80 != 0 {
        usize::from(props.get(1).copied().unwrap_or(0) >> 4) + 1
    } else {
        0
    };
    let iv_size = if first & 0x40 != 0 {
        usize::from(props.get(1).copied().unwrap_or(0) & 0x0f) + 1
    } else {
        0
    };
    let hdr_bytes = if (first & 0xc0) != 0 { 2 } else { 1 };
    let salt = props
        .get(hdr_bytes..hdr_bytes + salt_size)
        .ok_or_else(|| "Cannot open encrypted archive. Wrong password?".to_string())?;
    let iv_part = props
        .get(hdr_bytes + salt_size..hdr_bytes + salt_size + iv_size)
        .ok_or_else(|| "Cannot open encrypted archive. Wrong password?".to_string())?;
    let mut iv = [0u8; 16];
    iv[..iv_part.len().min(16)].copy_from_slice(&iv_part[..iv_part.len().min(16)]);

    let key: [u8; 32] = if num_cycles_power == 0x3f {
        let mut k = [0u8; 32];
        let mut pos = 0;
        for &b in salt {
            if pos < 32 {
                k[pos] = b;
                pos += 1;
            }
        }
        for u in password.encode_utf16() {
            for b in u.to_le_bytes() {
                if pos < 32 {
                    k[pos] = b;
                    pos += 1;
                }
            }
        }
        k
    } else {
        if num_cycles_power > 20 {
            return Err("Cannot open encrypted archive. Wrong password?".to_string());
        }
        let mut round_buf = Vec::with_capacity(salt.len() + password.len() * 2 + 8);
        round_buf.extend_from_slice(salt);
        for u in password.encode_utf16() {
            round_buf.extend_from_slice(&u.to_le_bytes());
        }
        let counter_offset = round_buf.len();
        round_buf.extend_from_slice(&[0u8; 8]);
        let rounds = 1u64 << num_cycles_power;
        let mut hasher = Sha256State::new();
        for round in 0..rounds {
            round_buf[counter_offset..counter_offset + 8].copy_from_slice(&round.to_le_bytes());
            hasher.update(&round_buf);
        }
        hasher.finalize()
    };
    Ok((key, iv))
}

fn sz_decrypt_aes256(props: &[u8], data: &[u8], password: &str) -> Result<Vec<u8>, String> {
    let (key, iv) = sz_derive_aes256_key_iv(props, password)?;
    zipfast::crypto::decrypt_cbc(&key, &iv, data)
        .ok_or_else(|| "Data Error in encrypted file. Wrong password?".to_string())
}

fn sz_bcj_start(props: &[u8]) -> Result<u32, String> {
    match props {
        [] => Ok(0),
        [a, b, c, d] => Ok(u32::from_le_bytes([*a, *b, *c, *d])),
        _ => Err("7-Zip: Invalid BCJ start offset property".to_string()),
    }
}

fn sz_decode_bcj2_streaming(
    main_buf: &[u8],
    call_buf: &[u8],
    jump_buf: &[u8],
    range_buf: &[u8],
    unpack_size: u64,
    emit: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if range_buf.len() < 5 {
        return Err("7-Zip: Truncated BCJ2 range stream".to_string());
    }
    let mut range: u32 = 0xffff_ffff;
    let mut code: u32 = 0;
    for &b in &range_buf[..5] {
        code = (code << 8) | u32::from(b);
    }
    let mut range_pos = 5usize;
    let mut main_pos = 0usize;
    let mut call_pos = 0usize;
    let mut jump_pos = 0usize;
    let mut probs = [1024u16; 258];
    let mut chunk = Vec::with_capacity(64 * 1024);
    let mut total_emitted = 0u64;
    let mut prev_byte = 0u8;

    while total_emitted + (chunk.len() as u64) < unpack_size {
        let Some(&b) = main_buf.get(main_pos) else {
            return Err("7-Zip: BCJ2 main stream ended early".to_string());
        };
        main_pos += 1;
        chunk.push(b);
        if chunk.len() >= 64 * 1024 {
            emit(&chunk)?;
            total_emitted += chunk.len() as u64;
            chunk.clear();
        }
        if total_emitted + (chunk.len() as u64) >= unpack_size {
            break;
        }
        let prob_idx = if b == 0xe8 {
            usize::from(prev_byte)
        } else if b == 0xe9 {
            256
        } else if prev_byte == 0x0f && (b & 0xf0) == 0x80 {
            257
        } else {
            prev_byte = b;
            continue;
        };
        prev_byte = b;

        if range < (1 << 24) {
            let rb = range_buf
                .get(range_pos)
                .copied()
                .ok_or_else(|| "7-Zip: Truncated BCJ2 range stream".to_string())?;
            range_pos += 1;
            range <<= 8;
            code = (code << 8) | u32::from(rb);
        }
        let ttt = u32::from(probs[prob_idx]);
        let bound = (range >> 11) * ttt;
        if code < bound {
            range = bound;
            probs[prob_idx] = (ttt + ((2048 - ttt) >> 5)) as u16;
            continue;
        }
        range -= bound;
        code -= bound;
        probs[prob_idx] = (ttt - (ttt >> 5)) as u16;

        let val = if b == 0xe8 {
            let s = call_buf
                .get(call_pos..call_pos + 4)
                .ok_or_else(|| "7-Zip: Truncated BCJ2 call stream".to_string())?;
            call_pos += 4;
            u32::from_be_bytes(s.try_into().unwrap())
        } else {
            let s = jump_buf
                .get(jump_pos..jump_pos + 4)
                .ok_or_else(|| "7-Zip: Truncated BCJ2 jump stream".to_string())?;
            jump_pos += 4;
            u32::from_be_bytes(s.try_into().unwrap())
        };
        let cur_pos = (total_emitted + (chunk.len() as u64)) as u32;
        let ip = cur_pos.wrapping_add(4);
        let rel = val.wrapping_sub(ip);
        let rel_bytes = rel.to_le_bytes();
        let rem = unpack_size.saturating_sub(total_emitted + (chunk.len() as u64));
        let take_n = (rem.min(4)) as usize;
        chunk.extend_from_slice(&rel_bytes[..take_n]);
        prev_byte = rel_bytes[3];
        if chunk.len() >= 64 * 1024 {
            emit(&chunk)?;
            total_emitted += chunk.len() as u64;
            chunk.clear();
        }
    }

    if !chunk.is_empty() {
        emit(&chunk)?;
        total_emitted += chunk.len() as u64;
    }
    if total_emitted != unpack_size {
        return Err("7-Zip: BCJ2 decompression incomplete".to_string());
    }
    Ok(())
}

fn sz_decode_bcj2(
    main_buf: &[u8],
    call_buf: &[u8],
    jump_buf: &[u8],
    range_buf: &[u8],
    unpack_size: u64,
) -> Result<Vec<u8>, String> {
    let target = usize::try_from(unpack_size).map_err(|_| "Unpack size overflow")?;
    if unpack_size > SZ_MAX_DECODED {
        return Err("7-Zip: Folder exceeds maximum unpacked size".to_string());
    }
    let mut out = Vec::with_capacity(target.min(16 * 1024 * 1024));
    sz_decode_bcj2_streaming(
        main_buf,
        call_buf,
        jump_buf,
        range_buf,
        unpack_size,
        &mut |s| {
            out.extend_from_slice(s);
            Ok(())
        },
    )?;
    Ok(out)
}

fn sz_run_coder(
    id: &[u8],
    props: &[u8],
    input: &[u8],
    unpack_size: u64,
    password: &str,
) -> Result<Vec<u8>, String> {
    let target = usize::try_from(unpack_size).map_err(|_| "Unpack size overflow")?;
    if unpack_size > SZ_MAX_DECODED {
        return Err("7-Zip: Folder exceeds maximum unpacked size".to_string());
    }
    let init_cap = target.min(16 * 1024 * 1024);
    match id {
        [0x00] => {
            if input.len() < target {
                return Err("7-Zip: Stored stream shorter than unpacked size".to_string());
            }
            Ok(input[..target].to_vec())
        }
        [0x03, 0x01, 0x01] => {
            if props.is_empty() {
                return Err("7-Zip: Missing LZMA properties".to_string());
            }
            let lzma_props = lzma::Props::from_byte(props[0])
                .ok_or_else(|| "7-Zip: Invalid LZMA properties".to_string())?;
            let (out, _end) = lzma::lzma(input, lzma_props, Some(unpack_size), target);
            if out.len() != target {
                return Err("7-Zip: LZMA decompression incomplete".to_string());
            }
            Ok(out)
        }
        [0x21] => {
            let (out, _end) = lzma::lzma2(input, target);
            if out.len() != target {
                return Err("7-Zip: LZMA2 decompression incomplete".to_string());
            }
            Ok(out)
        }
        [0x03, 0x04, 0x01] => {
            if props.len() < 5 {
                return Err("7-Zip: Missing PPMd properties".to_string());
            }
            let order = props[0];
            let mem_size = u32::from_le_bytes([props[1], props[2], props[3], props[4]]);
            ppmd_h::decode_7z_ppmd(input, order, mem_size, target)
        }
        [0x04, 0x01, 0x08] => {
            use std::io::Read;
            let dec = flate2::read::DeflateDecoder::new(input);
            let mut out = Vec::with_capacity(init_cap);
            dec.take(unpack_size)
                .read_to_end(&mut out)
                .map_err(|e| e.to_string())?;
            if out.len() != target {
                return Err("7-Zip: Deflate decompression incomplete".to_string());
            }
            Ok(out)
        }
        [0x04, 0x02, 0x02] => {
            use std::io::Read;
            let dec = bzip2::bufread::MultiBzDecoder::new(input);
            let mut out = Vec::with_capacity(init_cap);
            dec.take(unpack_size)
                .read_to_end(&mut out)
                .map_err(|e| e.to_string())?;
            if out.len() != target {
                return Err("7-Zip: BZip2 decompression incomplete".to_string());
            }
            Ok(out)
        }
        [0x04, 0xf7, 0x11, 0x01] => {
            use std::io::Read;
            let dec = zstd::stream::read::Decoder::new(input).map_err(|e| e.to_string())?;
            let mut out = Vec::with_capacity(init_cap);
            dec.take(unpack_size)
                .read_to_end(&mut out)
                .map_err(|e| e.to_string())?;
            if out.len() != target {
                return Err("7-Zip: Zstd decompression incomplete".to_string());
            }
            Ok(out)
        }
        [0x06, 0xf1, 0x07, 0x01] => {
            let mut dec = sz_decrypt_aes256(props, input, password)?;
            if dec.len() < target {
                return Err("Data Error in encrypted file. Wrong password?".to_string());
            }
            dec.truncate(target);
            Ok(dec)
        }
        [0x03] => {
            if input.len() < target {
                return Err("7-Zip: Delta input stream shorter than unpacked size".to_string());
            }
            let mut out = input[..target].to_vec();
            let dist = props.first().map_or(1, |&d| usize::from(d) + 1);
            let filter = bcj::Filter::Delta { dist };
            filter.undo(&mut out);
            Ok(out)
        }
        [0x03, 0x03, 0x01, 0x03] => {
            if input.len() < target {
                return Err("7-Zip: BCJ input stream shorter than unpacked size".to_string());
            }
            let mut out = input[..target].to_vec();
            let filter = bcj::Filter::Bcj {
                arch: bcj::Arch::X86,
                start: sz_bcj_start(props)?,
            };
            filter.undo(&mut out);
            Ok(out)
        }
        [0x03, 0x03, 0x02, 0x05] => {
            if input.len() < target {
                return Err("7-Zip: BCJ input stream shorter than unpacked size".to_string());
            }
            let mut out = input[..target].to_vec();
            let filter = bcj::Filter::Bcj {
                arch: bcj::Arch::PowerPc,
                start: sz_bcj_start(props)?,
            };
            filter.undo(&mut out);
            Ok(out)
        }
        [0x03, 0x03, 0x04, 0x01] => {
            if input.len() < target {
                return Err("7-Zip: BCJ input stream shorter than unpacked size".to_string());
            }
            let mut out = input[..target].to_vec();
            let filter = bcj::Filter::Bcj {
                arch: bcj::Arch::Ia64,
                start: sz_bcj_start(props)?,
            };
            filter.undo(&mut out);
            Ok(out)
        }
        [0x03, 0x03, 0x05, 0x01] => {
            if input.len() < target {
                return Err("7-Zip: BCJ input stream shorter than unpacked size".to_string());
            }
            let mut out = input[..target].to_vec();
            let filter = bcj::Filter::Bcj {
                arch: bcj::Arch::Arm,
                start: sz_bcj_start(props)?,
            };
            filter.undo(&mut out);
            Ok(out)
        }
        [0x03, 0x03, 0x07, 0x01] => {
            if input.len() < target {
                return Err("7-Zip: BCJ input stream shorter than unpacked size".to_string());
            }
            let mut out = input[..target].to_vec();
            let filter = bcj::Filter::Bcj {
                arch: bcj::Arch::ArmThumb,
                start: sz_bcj_start(props)?,
            };
            filter.undo(&mut out);
            Ok(out)
        }
        [0x03, 0x03, 0x08, 0x05] => {
            if input.len() < target {
                return Err("7-Zip: BCJ input stream shorter than unpacked size".to_string());
            }
            let mut out = input[..target].to_vec();
            let filter = bcj::Filter::Bcj {
                arch: bcj::Arch::Sparc,
                start: sz_bcj_start(props)?,
            };
            filter.undo(&mut out);
            Ok(out)
        }
        [0x0a] => {
            if input.len() < target {
                return Err("7-Zip: BCJ input stream shorter than unpacked size".to_string());
            }
            let mut out = input[..target].to_vec();
            let filter = bcj::Filter::Bcj {
                arch: bcj::Arch::Arm64,
                start: sz_bcj_start(props)?,
            };
            filter.undo(&mut out);
            Ok(out)
        }
        _ => Err(format!("7-Zip: Unsupported coder ID {id:02x?}")),
    }
}

fn sz_bcj_filter_from_coder(id: &[u8], props: &[u8]) -> Result<Option<bcj::Filter>, String> {
    match id {
        [0x03] => {
            let dist = props.first().map_or(1, |&d| usize::from(d) + 1);
            Ok(Some(bcj::Filter::Delta { dist }))
        }
        [0x03, 0x03, 0x01, 0x03] => Ok(Some(bcj::Filter::Bcj {
            arch: bcj::Arch::X86,
            start: sz_bcj_start(props)?,
        })),
        [0x03, 0x03, 0x02, 0x05] => Ok(Some(bcj::Filter::Bcj {
            arch: bcj::Arch::PowerPc,
            start: sz_bcj_start(props)?,
        })),
        [0x03, 0x03, 0x04, 0x01] => Ok(Some(bcj::Filter::Bcj {
            arch: bcj::Arch::Ia64,
            start: sz_bcj_start(props)?,
        })),
        [0x03, 0x03, 0x05, 0x01] => Ok(Some(bcj::Filter::Bcj {
            arch: bcj::Arch::Arm,
            start: sz_bcj_start(props)?,
        })),
        [0x03, 0x03, 0x07, 0x01] => Ok(Some(bcj::Filter::Bcj {
            arch: bcj::Arch::ArmThumb,
            start: sz_bcj_start(props)?,
        })),
        [0x03, 0x03, 0x08, 0x05] => Ok(Some(bcj::Filter::Bcj {
            arch: bcj::Arch::Sparc,
            start: sz_bcj_start(props)?,
        })),
        [0x0a] => Ok(Some(bcj::Filter::Bcj {
            arch: bcj::Arch::Arm64,
            start: sz_bcj_start(props)?,
        })),
        _ => Ok(None),
    }
}

fn sz_lzma2_dict_cap(props: &[u8]) -> Result<usize, String> {
    let Some(&p) = props.first() else {
        return Ok(8 << 20);
    };
    let bits = p & 0x3f;
    if bits > 40 {
        return Err("7-Zip: Invalid LZMA2 dictionary bits".to_string());
    }
    if bits == 40 {
        return Ok(usize::try_from(u32::MAX).unwrap_or(usize::MAX));
    }
    let base = 2usize | (usize::from(bits) & 1);
    Ok(base
        .checked_shl(u32::from(bits / 2 + 11))
        .unwrap_or(usize::MAX)
        .max(64 * 1024))
}

fn sz_decode_folder_streaming(
    folder: &SzFolder,
    packed_slices: &[&[u8]],
    password: &str,
    emit: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if folder.coders.is_empty() {
        return Err("7-Zip: Empty folder coders".to_string());
    }
    let encrypted = folder
        .coders
        .iter()
        .any(|c| c.id.as_slice() == [0x06, 0xf1, 0x07, 0x01]);
    let final_idx = folder
        .final_out_stream_index()
        .ok_or_else(|| "7-Zip: Missing final output stream in folder".to_string())?;

    let mut in_bases = Vec::with_capacity(folder.coders.len());
    let mut out_bases = Vec::with_capacity(folder.coders.len());
    let (mut cur_in, mut cur_out) = (0usize, 0usize);
    for c in &folder.coders {
        in_bases.push(cur_in);
        out_bases.push(cur_out);
        cur_in += c.num_in;
        cur_out += c.num_out;
    }

    let mut out_bufs: Vec<Option<Vec<u8>>> = vec![None; cur_out];
    let mut done = vec![false; folder.coders.len()];

    let can_resolve_input = |in_idx: usize, out_bufs: &[Option<Vec<u8>>]| -> bool {
        if let Some(&(_, out_idx)) = folder.bind_pairs.iter().find(|&&(i, _)| i == in_idx as u64) {
            return out_bufs.get(out_idx as usize).is_some_and(Option::is_some);
        }
        if let Some(p_idx) = folder
            .packed_streams
            .iter()
            .position(|&i| i == in_idx as u64)
        {
            return packed_slices.get(p_idx).is_some();
        }
        if folder.packed_streams.is_empty() && in_idx == 0 {
            return !packed_slices.is_empty();
        }
        false
    };

    let take_input = |in_idx: usize, out_bufs: &mut [Option<Vec<u8>>]| -> Option<Vec<u8>> {
        if let Some(&(_, out_idx)) = folder.bind_pairs.iter().find(|&&(i, _)| i == in_idx as u64) {
            return out_bufs.get_mut(out_idx as usize)?.take();
        }
        if let Some(p_idx) = folder
            .packed_streams
            .iter()
            .position(|&i| i == in_idx as u64)
        {
            return packed_slices.get(p_idx).map(|s| s.to_vec());
        }
        if folder.packed_streams.is_empty() && in_idx == 0 {
            return packed_slices.first().map(|s| s.to_vec());
        }
        None
    };

    let mut folder_crc_hasher = folder.crc.map(|_| crc32fast::Hasher::new());
    let mut streamed_final = false;

    for _ in 0..folder.coders.len() {
        let mut progressed = false;
        for c_idx in 0..folder.coders.len() {
            if done[c_idx] {
                continue;
            }
            let c = &folder.coders[c_idx];
            let in_base = in_bases[c_idx];
            let out_base = out_bases[c_idx];
            if !(0..c.num_in).all(|offset| can_resolve_input(in_base + offset, &out_bufs)) {
                continue;
            }
            let mut inputs = Vec::with_capacity(c.num_in);
            for offset in 0..c.num_in {
                let inp = take_input(in_base + offset, &mut out_bufs)
                    .ok_or_else(|| "7-Zip: Corrupt or duplicate coder input binding".to_string())?;
                inputs.push(inp);
            }
            let unpack_sz = folder.unpack.get(out_base).copied().unwrap_or(0);
            if out_base == final_idx
                && c.id.as_slice() == [0x03, 0x03, 0x01, 0x1b]
                && c.num_in == 4
                && c.num_out == 1
            {
                sz_decode_bcj2_streaming(
                    &inputs[0],
                    &inputs[1],
                    &inputs[2],
                    &inputs[3],
                    unpack_sz,
                    &mut |chunk| {
                        if let Some(h) = folder_crc_hasher.as_mut() {
                            h.update(chunk);
                        }
                        emit(chunk)
                    },
                )
                .map_err(|e| {
                    if encrypted {
                        "Data Error in encrypted file. Wrong password?".to_string()
                    } else {
                        e
                    }
                })?;
                streamed_final = true;
            } else {
                let decoded = if c.num_in == 1 && c.num_out == 1 {
                    sz_run_coder(&c.id, &c.props, &inputs[0], unpack_sz, password)
                } else if c.id.as_slice() == [0x03, 0x03, 0x01, 0x1b]
                    && c.num_in == 4
                    && c.num_out == 1
                {
                    sz_decode_bcj2(&inputs[0], &inputs[1], &inputs[2], &inputs[3], unpack_sz)
                } else {
                    Err(format!(
                        "7-Zip: Unsupported multi-stream coder {:02x?} ({}->{})",
                        c.id, c.num_in, c.num_out
                    ))
                }
                .map_err(|e| {
                    if encrypted {
                        "Data Error in encrypted file. Wrong password?".to_string()
                    } else {
                        e
                    }
                })?;
                out_bufs[out_base] = Some(decoded);
            }
            done[c_idx] = true;
            progressed = true;
        }
        if done.iter().all(|&d| d) {
            break;
        }
        if !progressed {
            return Err("7-Zip: Unresolvable coder bind pairs in folder".to_string());
        }
    }

    if !streamed_final {
        let buf = out_bufs
            .get_mut(final_idx)
            .and_then(Option::take)
            .ok_or_else(|| "7-Zip: Folder produced no output".to_string())?;
        if let Some(h) = folder_crc_hasher.as_mut() {
            h.update(&buf);
        }
        for chunk in buf.chunks(64 * 1024) {
            emit(chunk)?;
        }
    }

    if let (Some(expected_crc), Some(h)) = (folder.crc, folder_crc_hasher) {
        if h.finalize() != expected_crc {
            return Err(if encrypted {
                "CRC Failed in encrypted file. Wrong password?".to_string()
            } else {
                "7-Zip: Folder CRC failed".to_string()
            });
        }
    }
    Ok(())
}

fn sz_decode_folder(
    folder: &SzFolder,
    packed_slices: &[&[u8]],
    password: &str,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    sz_decode_folder_streaming(folder, packed_slices, password, &mut |chunk| {
        out.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(out)
}

fn sz_read_names(data: &[u8], count: u64) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(usize::try_from(count).ok()?);
    let mut units = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c));
    for _ in 0..count {
        let mut name = Vec::new();
        loop {
            let unit = units.next()?;
            if unit == 0 {
                break;
            }
            name.push(unit);
        }
        out.push(encode_utf16_injective(&name));
    }
    Some(out)
}

struct SzVolumePart {
    path: PathBuf,
    start_offset: u64,
    len: u64,
}

struct SzVolumeSet {
    parts: Vec<SzVolumePart>,
    total_len: u64,
    cached_file: Option<(usize, std::fs::File)>,
}

impl SzVolumeSet {
    fn open(archive_path: &Path) -> Result<Self, String> {
        let name = archive_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let mut parts = Vec::new();
        let mut total_len = 0u64;
        if name.to_ascii_lowercase().ends_with(".7z.001") {
            let prefix = &name[..name.len() - 3];
            let parent = archive_path.parent().unwrap_or_else(|| Path::new("."));
            for n in 1..1000u32 {
                let part_path = parent.join(format!("{prefix}{n:03}"));
                if !part_path.exists() {
                    if n == 1 {
                        return Err(format!("7-Zip: Volume {} not found", part_path.display()));
                    }
                    break;
                }
                let len = std::fs::metadata(&part_path)
                    .map_err(|e| e.to_string())?
                    .len();
                parts.push(SzVolumePart {
                    path: part_path,
                    start_offset: total_len,
                    len,
                });
                total_len = total_len.saturating_add(len);
            }
        } else {
            let len = std::fs::metadata(archive_path)
                .map_err(|e| e.to_string())?
                .len();
            parts.push(SzVolumePart {
                path: archive_path.to_path_buf(),
                start_offset: 0,
                len,
            });
            total_len = len;
        }
        Ok(Self {
            parts,
            total_len,
            cached_file: None,
        })
    }

    fn read_exact_at(&mut self, mut offset: u64, mut dst: &mut [u8]) -> Result<(), String> {
        let end = offset
            .checked_add(dst.len() as u64)
            .ok_or_else(|| "7-Zip: Read range overflow".to_string())?;
        if end > self.total_len {
            return Err("7-Zip: Unexpected EOF".to_string());
        }
        while !dst.is_empty() {
            let (idx, part_start, part_len) = self
                .parts
                .iter()
                .enumerate()
                .find(|(_, p)| offset >= p.start_offset && offset < p.start_offset + p.len)
                .map(|(i, p)| (i, p.start_offset, p.len))
                .ok_or_else(|| "7-Zip: Unexpected EOF".to_string())?;
            let need_open =
                !matches!(&self.cached_file, Some((cached_idx, _)) if *cached_idx == idx);
            if need_open {
                let f = std::fs::File::open(&self.parts[idx].path).map_err(|e| e.to_string())?;
                self.cached_file = Some((idx, f));
            }
            let file = &mut self.cached_file.as_mut().unwrap().1;
            let rel_offset = offset - part_start;
            file.seek(SeekFrom::Start(rel_offset))
                .map_err(|e| e.to_string())?;
            let avail = usize::try_from((part_start + part_len) - offset).unwrap_or(usize::MAX);
            let to_read = dst.len().min(avail);
            file.read_exact(&mut dst[..to_read])
                .map_err(|e| e.to_string())?;
            offset += to_read as u64;
            dst = &mut dst[to_read..];
        }
        Ok(())
    }

    fn read_vec_at(&mut self, offset: u64, len: u64) -> Result<Vec<u8>, String> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| "7-Zip: Range overflow".to_string())?;
        if end > self.total_len {
            return Err("7-Zip: Truncated archive".to_string());
        }
        let len_usize = usize::try_from(len).map_err(|_| "7-Zip: Size overflow".to_string())?;
        let mut buf = vec![0u8; len_usize];
        self.read_exact_at(offset, &mut buf)?;
        Ok(buf)
    }
}

fn collect_folder_packed_vecs(
    vols: &mut SzVolumeSet,
    streams: &SzStreams,
    folder_idx: usize,
    pack_offset: &mut u64,
    pack_idx: &mut usize,
) -> Result<Vec<Vec<u8>>, String> {
    let folder = &streams.folders[folder_idx];
    let num_pack = folder.packed_streams.len().max(1);
    let mut vecs = Vec::with_capacity(num_pack);
    for _ in 0..num_pack {
        let pack_len = streams.pack_sizes.get(*pack_idx).copied().unwrap_or(0);
        *pack_idx += 1;
        let v = vols
            .read_vec_at(*pack_offset, pack_len)
            .map_err(|_| "7-Zip: Truncated packed stream".to_string())?;
        vecs.push(v);
        *pack_offset = pack_offset.saturating_add(pack_len);
    }
    Ok(vecs)
}

struct SzVolumeSliceReader<'a> {
    vols: &'a mut SzVolumeSet,
    offset: u64,
    rem: u64,
}

impl std::io::Read for SzVolumeSliceReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.rem == 0 || buf.is_empty() {
            return Ok(0);
        }
        let step = buf
            .len()
            .min(usize::try_from(self.rem).unwrap_or(usize::MAX));
        self.vols
            .read_exact_at(self.offset, &mut buf[..step])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.offset += step as u64;
        self.rem -= step as u64;
        Ok(step)
    }
}

struct SzAes256Reader<'a> {
    inner: Box<dyn std::io::Read + 'a>,
    key: [u8; 32],
    iv: [u8; 16],
    rem_out: u64,
    raw_buf: Vec<u8>,
    dec_buf: Vec<u8>,
    dec_pos: usize,
}

impl<'a> SzAes256Reader<'a> {
    fn new(inner: Box<dyn std::io::Read + 'a>, key: [u8; 32], iv: [u8; 16], rem_out: u64) -> Self {
        Self {
            inner,
            key,
            iv,
            rem_out,
            raw_buf: vec![0u8; 64 * 1024],
            dec_buf: Vec::new(),
            dec_pos: 0,
        }
    }
}

impl std::io::Read for SzAes256Reader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        while self.dec_pos >= self.dec_buf.len() {
            if self.rem_out == 0 {
                return Ok(0);
            }
            let mut got = 0usize;
            while got < self.raw_buf.len() {
                match self.inner.read(&mut self.raw_buf[got..]) {
                    Ok(0) => break,
                    Ok(n) => got += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
            if got == 0 {
                return Ok(0);
            }
            if !got.is_multiple_of(16) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Data Error in encrypted file. Wrong password?",
                ));
            }
            let next_iv: [u8; 16] = self.raw_buf[got - 16..got].try_into().unwrap();
            let Some(mut dec) =
                zipfast::crypto::decrypt_cbc(&self.key, &self.iv, &self.raw_buf[..got])
            else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Data Error in encrypted file. Wrong password?",
                ));
            };
            self.iv = next_iv;
            let keep = dec
                .len()
                .min(usize::try_from(self.rem_out).unwrap_or(usize::MAX));
            dec.truncate(keep);
            self.rem_out -= keep as u64;
            self.dec_buf = dec;
            self.dec_pos = 0;
        }
        let avail = self.dec_buf.len() - self.dec_pos;
        let step = buf.len().min(avail);
        buf[..step].copy_from_slice(&self.dec_buf[self.dec_pos..self.dec_pos + step]);
        self.dec_pos += step;
        Ok(step)
    }
}

fn sz_try_stream_linear_folder(
    vols: &mut SzVolumeSet,
    folder: &SzFolder,
    pack_offset: u64,
    pack_len: u64,
    password: &str,
    emit: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<bool, String> {
    use std::io::Read;

    if folder.coders.is_empty()
        || folder.packed_streams.len() > 1
        || folder
            .coders
            .iter()
            .any(|c| c.num_in != 1 || c.num_out != 1)
    {
        return Ok(false);
    }

    let start_in = folder.packed_streams.first().copied().unwrap_or(0) as usize;
    if start_in >= folder.coders.len() {
        return Ok(false);
    }

    let mut chain: Vec<(usize, &SzCoder, u64)> = Vec::with_capacity(folder.coders.len());
    let mut visited = vec![false; folder.coders.len()];
    let mut cur_c = start_in;
    for step in 0..folder.coders.len() {
        if cur_c >= folder.coders.len() || visited[cur_c] {
            return Ok(false);
        }
        visited[cur_c] = true;
        let unpack_sz = folder.unpack.get(cur_c).copied().unwrap_or(0);
        chain.push((cur_c, &folder.coders[cur_c], unpack_sz));
        if step + 1 < folder.coders.len() {
            let Some(&(next_in, _)) = folder
                .bind_pairs
                .iter()
                .find(|&&(_, out_idx)| out_idx == cur_c as u64)
            else {
                return Ok(false);
            };
            cur_c = next_in as usize;
        }
    }

    let mut aes_prefix = 0usize;
    while aes_prefix < chain.len() && chain[aes_prefix].1.id.as_slice() == [0x06, 0xf1, 0x07, 0x01]
    {
        aes_prefix += 1;
    }

    let mut filter_suffix = 0usize;
    while filter_suffix < chain.len() - aes_prefix {
        let idx = chain.len() - 1 - filter_suffix;
        if sz_bcj_filter_from_coder(&chain[idx].1.id, &chain[idx].1.props)?.is_some() {
            filter_suffix += 1;
        } else {
            break;
        }
    }

    let mid = &chain[aes_prefix..chain.len() - filter_suffix];
    if mid.len() > 1 {
        return Ok(false);
    }
    if let Some((_, decomp_coder, _)) = mid.first() {
        let supported = matches!(
            decomp_coder.id.as_slice(),
            [0x00]
                | [0x03, 0x01, 0x01]
                | [0x21]
                | [0x04, 0x01, 0x08]
                | [0x04, 0x02, 0x02]
                | [0x04, 0xf7, 0x11, 0x01]
        );
        if !supported {
            return Ok(false);
        }
    }

    let end = pack_offset
        .checked_add(pack_len)
        .ok_or_else(|| "7-Zip: Pack range overflow".to_string())?;
    if end > vols.total_len {
        return Err("7-Zip: Truncated packed stream".to_string());
    }

    let encrypted = aes_prefix > 0;
    let map_err = |e: String| -> String {
        if encrypted {
            "Data Error in encrypted file. Wrong password?".to_string()
        } else {
            e
        }
    };

    let mut stream_filters: Vec<bcj::StreamFilter> = Vec::with_capacity(filter_suffix);
    for &(_, fcoder, _) in &chain[chain.len() - filter_suffix..] {
        let f = sz_bcj_filter_from_coder(&fcoder.id, &fcoder.props)?.unwrap();
        stream_filters.push(bcj::StreamFilter::new(f));
    }

    let mut folder_crc_hasher = folder.crc.map(|_| crc32fast::Hasher::new());
    let mut stage_a = Vec::new();
    let mut stage_b = Vec::new();
    let mut total_final = 0u64;
    let final_target = chain.last().map(|x| x.2).unwrap_or(0);

    let mut push_filtered = |chunk: &[u8]| -> Result<(), String> {
        if chunk.is_empty() {
            return Ok(());
        }
        if stream_filters.is_empty() {
            let rem = final_target.saturating_sub(total_final);
            let take_n = chunk.len().min(usize::try_from(rem).unwrap_or(usize::MAX));
            if take_n > 0 {
                if let Some(h) = folder_crc_hasher.as_mut() {
                    h.update(&chunk[..take_n]);
                }
                emit(&chunk[..take_n])?;
                total_final += take_n as u64;
            }
            return Ok(());
        }
        stage_a.clear();
        stage_a.extend_from_slice(chunk);
        for sf in &mut stream_filters {
            stage_b.clear();
            sf.push(&stage_a, &mut stage_b);
            std::mem::swap(&mut stage_a, &mut stage_b);
        }
        let rem = final_target.saturating_sub(total_final);
        let take_n = stage_a
            .len()
            .min(usize::try_from(rem).unwrap_or(usize::MAX));
        if take_n > 0 {
            if let Some(h) = folder_crc_hasher.as_mut() {
                h.update(&stage_a[..take_n]);
            }
            emit(&stage_a[..take_n])?;
            total_final += take_n as u64;
        }
        Ok(())
    };

    let run_res: Result<(), String> = (|| {
        let mut reader: Box<dyn Read + '_> = Box::new(SzVolumeSliceReader {
            vols,
            offset: pack_offset,
            rem: pack_len,
        });
        for &(_, aes_coder, aes_unpack) in &chain[..aes_prefix] {
            let (key, iv) = sz_derive_aes256_key_iv(&aes_coder.props, password)?;
            reader = Box::new(SzAes256Reader::new(reader, key, iv, aes_unpack));
        }

        match mid.first() {
            None => {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    push_filtered(&buf[..n])?;
                }
            }
            Some(&(_, decomp_coder, decomp_unpack)) => match decomp_coder.id.as_slice() {
                [0x00] => {
                    let mut buf = vec![0u8; 64 * 1024];
                    let mut left = decomp_unpack;
                    while left > 0 {
                        let want = (left as usize).min(buf.len());
                        let n = reader.read(&mut buf[..want]).map_err(|e| e.to_string())?;
                        if n == 0 {
                            return Err(
                                "7-Zip: Stored stream shorter than unpacked size".to_string()
                            );
                        }
                        left -= n as u64;
                        push_filtered(&buf[..n])?;
                    }
                }
                [0x03, 0x01, 0x01] => {
                    if decomp_coder.props.is_empty() {
                        return Err("7-Zip: Missing LZMA properties".to_string());
                    }
                    let lzma_props = lzma::Props::from_byte(decomp_coder.props[0])
                        .ok_or_else(|| "7-Zip: Invalid LZMA properties".to_string())?;
                    let dict_cap = decomp_coder
                        .props
                        .get(1..5)
                        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                        .unwrap_or(8 << 20)
                        .max(64 * 1024);
                    lzma::lzma_from_reader(
                        &mut reader,
                        lzma_props,
                        dict_cap,
                        Some(decomp_unpack),
                        &mut push_filtered,
                    )?;
                }
                [0x21] => {
                    let dict_cap = sz_lzma2_dict_cap(&decomp_coder.props)?;
                    let mut got_unpacked = 0u64;
                    lzma::lzma2_from_reader(&mut reader, dict_cap, &mut |chunk| {
                        let rem = decomp_unpack.saturating_sub(got_unpacked);
                        let take_n = chunk.len().min(usize::try_from(rem).unwrap_or(usize::MAX));
                        got_unpacked += take_n as u64;
                        if take_n > 0 {
                            push_filtered(&chunk[..take_n])?;
                        }
                        Ok(())
                    })?;
                    if got_unpacked != decomp_unpack {
                        return Err("7-Zip: LZMA2 decompression incomplete".to_string());
                    }
                }
                [0x04, 0x01, 0x08] => {
                    let mut dec = flate2::read::DeflateDecoder::new(reader).take(decomp_unpack);
                    let mut buf = vec![0u8; 64 * 1024];
                    let mut got = 0u64;
                    loop {
                        let n = dec.read(&mut buf).map_err(|e| e.to_string())?;
                        if n == 0 {
                            break;
                        }
                        got += n as u64;
                        push_filtered(&buf[..n])?;
                    }
                    if got != decomp_unpack {
                        return Err("7-Zip: Deflate decompression incomplete".to_string());
                    }
                }
                [0x04, 0x02, 0x02] => {
                    let mut dec = bzip2::read::MultiBzDecoder::new(reader).take(decomp_unpack);
                    let mut buf = vec![0u8; 64 * 1024];
                    let mut got = 0u64;
                    loop {
                        let n = dec.read(&mut buf).map_err(|e| e.to_string())?;
                        if n == 0 {
                            break;
                        }
                        got += n as u64;
                        push_filtered(&buf[..n])?;
                    }
                    if got != decomp_unpack {
                        return Err("7-Zip: BZip2 decompression incomplete".to_string());
                    }
                }
                [0x04, 0xf7, 0x11, 0x01] => {
                    let mut dec = zstd::stream::read::Decoder::new(reader)
                        .map_err(|e| e.to_string())?
                        .take(decomp_unpack);
                    let mut buf = vec![0u8; 64 * 1024];
                    let mut got = 0u64;
                    loop {
                        let n = dec.read(&mut buf).map_err(|e| e.to_string())?;
                        if n == 0 {
                            break;
                        }
                        got += n as u64;
                        push_filtered(&buf[..n])?;
                    }
                    if got != decomp_unpack {
                        return Err("7-Zip: Zstd decompression incomplete".to_string());
                    }
                }
                _ => unreachable!(),
            },
        }
        Ok(())
    })();
    run_res.map_err(map_err)?;

    if !stream_filters.is_empty() {
        stage_a.clear();
        for idx in 0..stream_filters.len() {
            stage_b.clear();
            if !stage_a.is_empty() {
                stream_filters[idx].push(&stage_a, &mut stage_b);
            }
            stream_filters[idx].finish(&mut stage_b);
            std::mem::swap(&mut stage_a, &mut stage_b);
        }
        let rem = final_target.saturating_sub(total_final);
        let take_n = stage_a
            .len()
            .min(usize::try_from(rem).unwrap_or(usize::MAX));
        if take_n > 0 {
            if let Some(h) = folder_crc_hasher.as_mut() {
                h.update(&stage_a[..take_n]);
            }
            emit(&stage_a[..take_n])?;
            total_final += take_n as u64;
        }
    }

    if total_final != final_target {
        return Err(map_err("7-Zip: Decompression incomplete".to_string()));
    }
    if let (Some(expected_crc), Some(h)) = (folder.crc, folder_crc_hasher) {
        if h.finalize() != expected_crc {
            return Err(if encrypted {
                "CRC Failed in encrypted file. Wrong password?".to_string()
            } else {
                "7-Zip: Folder CRC failed".to_string()
            });
        }
    }
    Ok(true)
}

/// Extracts a `.7z` or `.7z.001` archive into `output_dir` using pure Rust.
pub fn extract_7z<F>(
    archive_path: &Path,
    output_dir: &Path,
    password: &str,
    overwrite_mode: i32,
    log_cb: &mut F,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    let mut never_cancel = || false;
    extract_7z_cancellable(
        archive_path,
        output_dir,
        password,
        overwrite_mode,
        log_cb,
        &mut never_cancel,
    )
}

/// Extracts a `.7z` or `.7z.001` archive into `output_dir` using pure Rust,
/// polling `is_cancelled` during extraction so callers can abort cleanly.
pub fn extract_7z_cancellable<F>(
    archive_path: &Path,
    output_dir: &Path,
    password: &str,
    overwrite_mode: i32,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    if is_cancelled() {
        return Err("7-Zip: Aborted".to_string());
    }
    let mut vols = SzVolumeSet::open(archive_path).map_err(|e| {
        log_cb(1, &e);
        e
    })?;
    if vols.total_len < 32 {
        let msg = "7-Zip: Not a valid 7z archive".to_string();
        log_cb(1, &msg);
        return Err(msg);
    }
    let mut sig_hdr = [0u8; 32];
    vols.read_exact_at(0, &mut sig_hdr).map_err(|e| {
        log_cb(1, &e);
        e
    })?;
    if !sig_hdr.starts_with(SEVEN_Z) {
        let msg = "7-Zip: Not a valid 7z archive".to_string();
        log_cb(1, &msg);
        return Err(msg);
    }
    if le32(&sig_hdr, 8) != Some(crc32(&sig_hdr[12..32])) {
        let msg = "7-Zip: Signature header CRC mismatch".to_string();
        log_cb(1, &msg);
        return Err(msg);
    }
    let next_offset = le64(&sig_hdr, 12).unwrap_or(0);
    let next_size = le64(&sig_hdr, 20).unwrap_or(0);
    let next_crc = le32(&sig_hdr, 28).unwrap_or(0);

    if next_size == 0 {
        log_cb(4, "7-Zip: Everything is Ok");
        return Ok(zipfast::extract::Summary::default());
    }

    let hdr_start = 32u64
        .checked_add(next_offset)
        .ok_or_else(|| "Offset overflow".to_string())?;
    let mut hdr_bytes = vols.read_vec_at(hdr_start, next_size).map_err(|_| {
        let msg = "7-Zip: Truncated archive (missing end header)".to_string();
        log_cb(1, &msg);
        msg
    })?;
    if crc32(&hdr_bytes) != next_crc {
        let msg = "7-Zip: End header CRC mismatch".to_string();
        log_cb(1, &msg);
        return Err(msg);
    }

    for _ in 0..4 {
        let mut r = SzReader {
            b: &hdr_bytes,
            p: 0,
        };
        match r.byte() {
            Some(0x01) => break,
            Some(0x17) => {
                let Some(enc_streams) = sz_streams(&mut r) else {
                    let msg = "7-Zip: Corrupt EncodedHeader streams".to_string();
                    log_cb(1, &msg);
                    return Err(msg);
                };
                if enc_streams.folders.is_empty() {
                    let msg = "7-Zip: Empty EncodedHeader folder".to_string();
                    log_cb(1, &msg);
                    return Err(msg);
                }
                let mut pack_offset = 32u64.saturating_add(enc_streams.pack_pos);
                let mut pack_idx = 0usize;
                let packed_vecs = collect_folder_packed_vecs(
                    &mut vols,
                    &enc_streams,
                    0,
                    &mut pack_offset,
                    &mut pack_idx,
                )
                .map_err(|e| {
                    log_cb(1, &e);
                    e
                })?;
                let slice_refs: Vec<&[u8]> = packed_vecs.iter().map(Vec::as_slice).collect();
                hdr_bytes = sz_decode_folder(&enc_streams.folders[0], &slice_refs, password)
                    .map_err(|e| {
                        log_cb(1, &e);
                        e
                    })?;
            }
            _ => {
                let msg = "7-Zip: Unexpected header marker".to_string();
                log_cb(1, &msg);
                return Err(msg);
            }
        }
    }

    let mut r = SzReader {
        b: &hdr_bytes,
        p: 0,
    };
    if r.byte() != Some(0x01) {
        let msg = "7-Zip: Missing Header marker (0x01)".to_string();
        log_cb(1, &msg);
        return Err(msg);
    }
    let mut id = r
        .byte()
        .ok_or_else(|| "7-Zip: Unexpected EOF in header".to_string())?;
    if id == 0x02 {
        while let Some(ty) = r.number() {
            if ty == 0 {
                break;
            }
            let sz = r
                .number()
                .ok_or_else(|| "7-Zip: Corrupt archive props".to_string())?;
            r.take(sz)
                .ok_or_else(|| "7-Zip: Corrupt archive props".to_string())?;
        }
        id = r
            .byte()
            .ok_or_else(|| "7-Zip: Unexpected EOF in header".to_string())?;
    }
    if id == 0x03 {
        sz_streams(&mut r).ok_or_else(|| "7-Zip: Corrupt AdditionalStreamsInfo".to_string())?;
        id = r
            .byte()
            .ok_or_else(|| "7-Zip: Unexpected EOF in header".to_string())?;
    }
    let mut main_streams = SzStreams::default();
    if id == 0x04 {
        main_streams =
            sz_streams(&mut r).ok_or_else(|| "7-Zip: Corrupt MainStreamsInfo".to_string())?;
        id = r
            .byte()
            .ok_or_else(|| "7-Zip: Unexpected EOF in header".to_string())?;
    }

    let mut empty_stream: Vec<bool> = Vec::new();
    let mut empty_file: Vec<bool> = Vec::new();
    let mut file_names: Vec<String> = Vec::new();
    let mut file_mtimes: Vec<Option<(i64, u32)>> = Vec::new();
    let mut win_attrs: Vec<Option<u32>> = Vec::new();
    if id == 0x05 {
        let count = r
            .number()
            .ok_or_else(|| "7-Zip: Corrupt FilesInfo count".to_string())?;
        if count > SZ_MAX_ENTRIES as u64 {
            let msg = "7-Zip: Archive exceeds maximum entry count".to_string();
            log_cb(1, &msg);
            return Err(msg);
        }
        loop {
            let ty = r
                .number()
                .ok_or_else(|| "7-Zip: Corrupt FilesInfo prop".to_string())?;
            if ty == 0 {
                break;
            }
            let sz = r
                .number()
                .ok_or_else(|| "7-Zip: Corrupt FilesInfo prop size".to_string())?;
            let mut part = SzReader {
                b: r.take(sz)
                    .ok_or_else(|| "7-Zip: Truncated FilesInfo prop".to_string())?,
                p: 0,
            };
            match ty {
                0x0e => {
                    empty_stream = part
                        .bits(count)
                        .ok_or_else(|| "7-Zip: Corrupt EmptyStream".to_string())?;
                }
                0x0f => {
                    let empties = empty_stream.iter().filter(|e| **e).count() as u64;
                    empty_file = part
                        .bits(empties)
                        .ok_or_else(|| "7-Zip: Corrupt EmptyFile".to_string())?;
                }
                0x11 => {
                    if part.byte() != Some(0) {
                        return Err("7-Zip: External file names not supported".to_string());
                    }
                    let rest = part.b.get(part.p..).unwrap_or_default();
                    file_names = sz_read_names(rest, count)
                        .ok_or_else(|| "7-Zip: Corrupt UTF-16LE file names".to_string())?;
                }
                0x14 => {
                    let defined = match part.byte() {
                        Some(0) => part
                            .bits(count)
                            .ok_or_else(|| "7-Zip: Corrupt MTime".to_string())?,
                        Some(_) => vec![true; count as usize],
                        None => return Err("7-Zip: Corrupt MTime".to_string()),
                    };
                    if part.byte() != Some(0) {
                        return Err("7-Zip: External MTime not supported".to_string());
                    }
                    const FILETIME_UNIX_DIFF_TICKS: i128 = 116_444_736_000_000_000;
                    file_mtimes = Vec::with_capacity(count as usize);
                    for d in defined {
                        if d {
                            let b = part
                                .take(8)
                                .ok_or_else(|| "7-Zip: Truncated MTime".to_string())?;
                            let ticks = u64::from_le_bytes(b.try_into().unwrap());
                            if ticks == 0 {
                                file_mtimes.push(None);
                            } else {
                                let unix_ticks = i128::from(ticks) - FILETIME_UNIX_DIFF_TICKS;
                                let secs = unix_ticks.div_euclid(10_000_000) as i64;
                                let nanos = (unix_ticks.rem_euclid(10_000_000) as u32) * 100;
                                file_mtimes.push(Some((secs, nanos)));
                            }
                        } else {
                            file_mtimes.push(None);
                        }
                    }
                }
                0x15 => {
                    let defined = match part.byte() {
                        Some(0) => part
                            .bits(count)
                            .ok_or_else(|| "7-Zip: Corrupt WinAttributes".to_string())?,
                        Some(_) => vec![true; count as usize],
                        None => return Err("7-Zip: Corrupt WinAttributes".to_string()),
                    };
                    if part.byte() != Some(0) {
                        return Err("7-Zip: External WinAttributes not supported".to_string());
                    }
                    win_attrs = Vec::with_capacity(count as usize);
                    for d in defined {
                        if d {
                            let b = part
                                .take(4)
                                .ok_or_else(|| "7-Zip: Truncated WinAttributes".to_string())?;
                            win_attrs.push(Some(u32::from_le_bytes(b.try_into().unwrap())));
                        } else {
                            win_attrs.push(None);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    for i in 0..file_names.len() {
        let attr = win_attrs.get(i).copied().flatten().unwrap_or(0);
        let unix_mode = attr >> 16;
        if (unix_mode & 0o170000) == 0o120000 || (attr & 0x0400) != 0 {
            log_cb(1, "ERROR: Dangerous link path was ignored");
            log_cb(0, "Stream repair: archive contains link");
            return Err("archive contains link".to_string());
        }
    }

    // Stream folders one by one directly to destination files on disk in bounded memory.
    let mut pack_offset = 32u64.saturating_add(main_streams.pack_pos);
    let mut pack_idx = 0usize;
    let mut cur_folder_idx = 0usize;

    let mut summary = zipfast::extract::Summary::default();
    let archive_end = hdr_start.saturating_add(next_size).max(32);
    summary.volumes = vols
        .parts
        .iter()
        .take_while(|p| p.start_offset < archive_end)
        .count()
        .max(1) as u64;
    let mut entry_cursor = 0usize;
    let mut stream_cursor = 0usize;
    let mut empty_cursor = 0usize;

    let advance_empty_entries = |entry_cursor: &mut usize,
                                 empty_cursor: &mut usize,
                                 summary: &mut zipfast::extract::Summary,
                                 log_cb: &mut F,
                                 is_cancelled: &mut dyn FnMut() -> bool|
     -> Result<(), String> {
        while *entry_cursor < file_names.len()
            && empty_stream.get(*entry_cursor).copied().unwrap_or(false)
        {
            if is_cancelled() {
                return Err("7-Zip: Aborted".to_string());
            }
            let i = *entry_cursor;
            *entry_cursor += 1;
            let name = &file_names[i];
            let attr = win_attrs.get(i).copied().flatten().unwrap_or(0);
            let unix_mode = attr >> 16;
            let rel = zipfast::path::sanitize(name, true).map_err(|e| {
                let msg = format!("7-Zip: {e}");
                log_cb(1, &msg);
                msg
            })?;
            let is_empty_file = empty_file.get(*empty_cursor).copied().unwrap_or(false);
            *empty_cursor += 1;
            if !is_empty_file {
                create_dir_all_no_follow(output_dir, &rel).map_err(|e| {
                    let msg = format!("7-Zip: {e}");
                    log_cb(1, &msg);
                    msg
                })?;
                summary.dirs += 1;
                continue;
            }
            log_cb(4, &format!("7-Zip: Extracting  {name}"));
            match create_dest_file_no_follow(output_dir, &rel, overwrite_mode).map_err(|e| {
                let msg = format!("7-Zip: {e}");
                log_cb(1, &msg);
                msg
            })? {
                Some(f) => {
                    if unix_mode != 0 {
                        apply_unix_executable_mode(&f, unix_mode);
                    }
                    if let Some((secs, nanos)) = file_mtimes.get(i).copied().flatten() {
                        set_file_mtime(&f, Some(secs), nanos);
                    }
                    summary.files += 1;
                }
                None => {
                    summary.skipped += 1;
                }
            }
        }
        Ok(())
    };

    struct SzSubTarget {
        name: String,
        rel: PathBuf,
        unix_mode: u32,
        mtime: Option<(i64, u32)>,
        size: u64,
        expected_crc: Option<u32>,
    }

    struct ActiveSubstream {
        target_idx: usize,
        rem: u64,
        hasher: crc32fast::Hasher,
        file: Option<std::fs::File>,
        target_path: Option<PathBuf>,
        created_new: bool,
    }

    impl ActiveSubstream {
        fn discard(&mut self) {
            drop(self.file.take());
            if let Some(p) = self.target_path.take() {
                if self.created_new {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }

    advance_empty_entries(
        &mut entry_cursor,
        &mut empty_cursor,
        &mut summary,
        log_cb,
        is_cancelled,
    )?;

    while entry_cursor < file_names.len() {
        if is_cancelled() {
            return Err("7-Zip: Aborted".to_string());
        }
        if cur_folder_idx >= main_streams.folders.len() {
            let msg = "7-Zip: Missing folder stream for file".to_string();
            log_cb(1, &msg);
            return Err(msg);
        }
        let f_idx = cur_folder_idx;
        cur_folder_idx += 1;
        let n_sub = main_streams.per_folder.get(f_idx).copied().unwrap_or(1) as usize;
        let folder = &main_streams.folders[f_idx];

        if n_sub == 0 {
            let num_pack = folder.packed_streams.len().max(1);
            for _ in 0..num_pack {
                let pack_len = main_streams.pack_sizes.get(pack_idx).copied().unwrap_or(0);
                pack_idx += 1;
                pack_offset = pack_offset
                    .checked_add(pack_len)
                    .ok_or_else(|| "7-Zip: Pack range overflow".to_string())?;
            }
            continue;
        }

        let mut sub_targets: Vec<SzSubTarget> = Vec::with_capacity(n_sub);
        for _ in 0..n_sub {
            advance_empty_entries(
                &mut entry_cursor,
                &mut empty_cursor,
                &mut summary,
                log_cb,
                is_cancelled,
            )?;
            if entry_cursor >= file_names.len() {
                break;
            }
            let i = entry_cursor;
            entry_cursor += 1;
            let name = file_names[i].clone();
            let attr = win_attrs.get(i).copied().flatten().unwrap_or(0);
            let unix_mode = attr >> 16;
            let rel = zipfast::path::sanitize(&name, true).map_err(|e| {
                let msg = format!("7-Zip: {e}");
                log_cb(1, &msg);
                msg
            })?;
            let sz_u64 = main_streams
                .sizes
                .get(stream_cursor)
                .copied()
                .ok_or_else(|| {
                    let msg = "7-Zip: Missing substream size for file".to_string();
                    log_cb(1, &msg);
                    msg
                })?;
            let expected_crc = main_streams.digests.get(stream_cursor).copied().flatten();
            let mtime = file_mtimes.get(i).copied().flatten();
            stream_cursor += 1;
            sub_targets.push(SzSubTarget {
                name,
                rel,
                unix_mode,
                mtime,
                size: sz_u64,
                expected_crc,
            });
        }

        let mut sub_idx = 0usize;
        let mut active: Option<ActiveSubstream> = None;

        let open_or_advance_substreams = |sub_idx: &mut usize,
                                          active: &mut Option<ActiveSubstream>,
                                          summary: &mut zipfast::extract::Summary,
                                          log_cb: &mut F|
         -> Result<(), String> {
            while active.is_none() && *sub_idx < sub_targets.len() {
                let idx = *sub_idx;
                let t = &sub_targets[idx];
                log_cb(4, &format!("7-Zip: Extracting  {}", t.name));
                let opened =
                    create_dest_file_with_path_no_follow(output_dir, &t.rel, overwrite_mode)
                        .map_err(|e| format!("7-Zip: {e}"))?;
                if t.size == 0 {
                    if let Some(crc) = t.expected_crc {
                        if crc != 0 {
                            if let Some((f, p, c)) = opened {
                                drop(f);
                                if c {
                                    let _ = std::fs::remove_file(p);
                                }
                            }
                            return Err(format!("7-Zip: {} - CRC failed", t.name));
                        }
                    }
                    match opened {
                        Some((f, _, _)) => {
                            if t.unix_mode != 0 {
                                apply_unix_executable_mode(&f, t.unix_mode);
                            }
                            if let Some((secs, nanos)) = t.mtime {
                                set_file_mtime(&f, Some(secs), nanos);
                            }
                            summary.files += 1;
                        }
                        None => {
                            summary.skipped += 1;
                        }
                    }
                    *sub_idx += 1;
                } else {
                    let (file, target_path, created_new) = match opened {
                        Some((f, p, c)) => (Some(f), Some(p), c),
                        None => (None, None, false),
                    };
                    *active = Some(ActiveSubstream {
                        target_idx: idx,
                        rem: t.size,
                        hasher: crc32fast::Hasher::new(),
                        file,
                        target_path,
                        created_new,
                    });
                }
            }
            Ok(())
        };

        let folder_res = (|| -> Result<(), String> {
            open_or_advance_substreams(&mut sub_idx, &mut active, &mut summary, log_cb)?;

            let mut emit_chunk = |mut chunk: &[u8]| -> Result<(), String> {
                if is_cancelled() {
                    return Err("7-Zip: Aborted".to_string());
                }
                while !chunk.is_empty() {
                    open_or_advance_substreams(&mut sub_idx, &mut active, &mut summary, log_cb)?;
                    let Some(act) = active.as_mut() else {
                        break;
                    };
                    let step = chunk
                        .len()
                        .min(usize::try_from(act.rem).unwrap_or(usize::MAX));
                    let part = &chunk[..step];
                    act.hasher.update(part);
                    if let Some(f) = act.file.as_mut() {
                        for sub in part.chunks(256 * 1024) {
                            if is_cancelled() {
                                return Err("7-Zip: Aborted".to_string());
                            }
                            f.write_all(sub).map_err(|e| e.to_string())?;
                        }
                    }
                    act.rem -= step as u64;
                    chunk = &chunk[step..];
                    if act.rem == 0 {
                        let mut finished = active.take().unwrap();
                        let t = &sub_targets[finished.target_idx];
                        if let Some(crc) = t.expected_crc {
                            if finished.hasher.clone().finalize() != crc {
                                finished.discard();
                                return Err(format!("7-Zip: {} - CRC failed", t.name));
                            }
                        }
                        match finished.file.take() {
                            Some(f) => {
                                if t.unix_mode != 0 {
                                    apply_unix_executable_mode(&f, t.unix_mode);
                                }
                                if let Some((secs, nanos)) = t.mtime {
                                    set_file_mtime(&f, Some(secs), nanos);
                                }
                                summary.files += 1;
                                summary.bytes += t.size;
                            }
                            None => {
                                summary.skipped += 1;
                            }
                        }
                        sub_idx += 1;
                        open_or_advance_substreams(
                            &mut sub_idx,
                            &mut active,
                            &mut summary,
                            log_cb,
                        )?;
                    }
                }
                Ok(())
            };

            let streamed = if folder.packed_streams.len() <= 1 {
                let pack_len = main_streams.pack_sizes.get(pack_idx).copied().unwrap_or(0);
                let folder_pack_offset = pack_offset;
                let ok = sz_try_stream_linear_folder(
                    &mut vols,
                    folder,
                    folder_pack_offset,
                    pack_len,
                    password,
                    &mut emit_chunk,
                )?;
                if ok {
                    pack_idx += 1;
                    pack_offset = folder_pack_offset
                        .checked_add(pack_len)
                        .ok_or_else(|| "7-Zip: Pack range overflow".to_string())?;
                }
                ok
            } else {
                false
            };

            if !streamed {
                let packed_vecs = collect_folder_packed_vecs(
                    &mut vols,
                    &main_streams,
                    f_idx,
                    &mut pack_offset,
                    &mut pack_idx,
                )?;
                let slice_refs: Vec<&[u8]> = packed_vecs.iter().map(Vec::as_slice).collect();
                sz_decode_folder_streaming(folder, &slice_refs, password, &mut emit_chunk)?;
            }

            open_or_advance_substreams(&mut sub_idx, &mut active, &mut summary, log_cb)?;
            if active.is_some() || sub_idx < sub_targets.len() {
                return Err("7-Zip: Decoded folder shorter than file substream size".to_string());
            }
            Ok(())
        })();

        if let Err(err) = folder_res {
            if let Some(mut act) = active.take() {
                act.discard();
            }
            if err != "7-Zip: Aborted" {
                log_cb(1, &err);
            }
            return Err(err);
        }

        advance_empty_entries(
            &mut entry_cursor,
            &mut empty_cursor,
            &mut summary,
            log_cb,
            is_cancelled,
        )?;
    }

    log_cb(4, "7-Zip: Everything is Ok");
    Ok(summary)
}

fn set_file_mtime(file: &std::fs::File, mtime: Option<i64>, nanos: u32) {
    if let Some(secs) = mtime {
        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        let at = if secs >= 0 {
            UNIX_EPOCH.checked_add(Duration::new(secs as u64, nanos.min(999_999_999)))
        } else {
            UNIX_EPOCH.checked_sub(Duration::from_secs(secs.unsigned_abs()))
        };
        if let Some(at) = at.filter(|t| *t != SystemTime::UNIX_EPOCH || secs == 0) {
            let _ = file.set_modified(at);
        }
    }
}

/// Extracts ZIP, TAR, or single-stream (`.gz`, `.bz2`, `.xz`, `.zst`) archives in-process
/// while polling `is_cancelled` on every block/chunk read and output write.
pub fn extract_zipfast_cancellable<F>(
    archive_path: &Path,
    output_dir: &Path,
    password: &str,
    overwrite_mode: i32,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    if is_cancelled() {
        return Err("Aborted".to_string());
    }

    let mut head = [0u8; 1024];
    let mut head_len = 0usize;
    if let Ok(mut f) = std::fs::File::open(archive_path) {
        while head_len < head.len() {
            match f.read(&mut head[head_len..]) {
                Ok(0) => break,
                Ok(k) => head_len += k,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    } else {
        return Err(format!("Could not open {}", archive_path.display()));
    }
    let head_slice = &head[..head_len];

    if zipfast::zip::sniff(head_slice) {
        return extract_zip_cancellable(
            archive_path,
            output_dir,
            password,
            overwrite_mode,
            log_cb,
            is_cancelled,
        );
    }
    if zipfast::stream::Codec::sniff(head_slice).is_some() || zipfast::tar::sniff_tar(head_slice) {
        return extract_tar_or_stream_cancellable(
            archive_path,
            output_dir,
            overwrite_mode,
            log_cb,
            is_cancelled,
        );
    }

    // RAR sets are read by `crate::progressive_unpack` before this point; anything else that
    // is not a stream or tar is tried as ZIP.
    extract_zip_cancellable(
        archive_path,
        output_dir,
        password,
        overwrite_mode,
        log_cb,
        is_cancelled,
    )
}

const ZIP_BAD_PASSWORD: &str = "Unrar: The specified password is incorrect.";

/// Whether a failed member only reports that the read was cancelled.
fn zip_error_is_cancel(error: &zipfast::zip::ZipReadError) -> bool {
    matches!(error.operation(), Some(zipfast::zip::ReadFault::Cancelled))
        || matches!(
            error.sink_fault(),
            Some(zipfast::zip::ZipSinkFault::Cancelled)
        )
}

/// How a failed ZIP member is reported, in RustyGet's existing wording. The last line is the
/// one returned as the error.
fn zip_member_failure_lines(name: &str, error: &zipfast::zip::ZipReadError) -> Vec<String> {
    use zipfast::zip::{ReadFault, SessionError, ZipSinkFault};
    if error.is_source_changed()
        || matches!(
            error.operation(),
            Some(ReadFault::Session(SessionError::SourceChanged { .. }))
        )
    {
        return vec![format!(
            "{name}: the archive changed while it was being read"
        )];
    }
    match error.operation() {
        Some(ReadFault::PasswordRequired) | Some(ReadFault::PasswordCheckFailed { .. }) => {
            vec![
                ZIP_BAD_PASSWORD.to_string(),
                format!("{name}: wrong password"),
            ]
        }
        // The password verifier already matched: a bad WinZip AES tag is damaged data.
        Some(ReadFault::AuthenticationFailed) | Some(ReadFault::Integrity { .. }) => {
            vec![format!("Unrar: {name} - CRC failed")]
        }
        Some(ReadFault::UnsupportedMethod { method }) => {
            vec![format!(
                "{name}: compression method {method} is not supported"
            )]
        }
        // Encrypted members over 64 KiB are spooled to the temporary folder before their tag
        // is checked, and zipfast only spools to a private folder on a local file system.
        Some(ReadFault::UnsupportedStorage) | Some(ReadFault::UnsupportedIdentity) => {
            vec![format!(
                "{name}: zipfast cannot spool this encrypted member in the temporary folder; \
             point TMPDIR at a private local folder (ext4, XFS, btrfs, tmpfs or APFS)"
            )]
        }
        Some(ReadFault::CheckedFormat { reason }) => vec![format!("{name}: {reason}")],
        Some(ReadFault::Limit { resource, .. }) => {
            vec![format!("{name}: exceeds the {resource:?} limit")]
        }
        Some(ReadFault::Io { error, .. }) => vec![format!("{name}: {error}")],
        Some(ReadFault::Directory { error }) => vec![format!("{name}: {error}")],
        Some(ReadFault::Legacy(error)) => vec![format!("{name}: {error}")],
        Some(ReadFault::Session(SessionError::Archive(error))) => match error {
            zipfast::Error::BadChecksum(_) => vec![format!("Unrar: {name} - CRC failed")],
            // The in-memory reader reports a wrong password or a damaged AES stream as one
            // error; RustyGet keys its password retry on this line.
            zipfast::Error::Corrupt(what) if what.contains("wrong password") => {
                vec![
                    ZIP_BAD_PASSWORD.to_string(),
                    format!("{name}: wrong password"),
                ]
            }
            other => vec![format!("{name}: {other}")],
        },
        Some(ReadFault::Session(error)) => vec![format!("{name}: {error}")],
        Some(ReadFault::Cancelled) => vec!["Aborted".to_string()],
        Some(other) => vec![format!("{name}: {other:?}")],
        None => match error.sink_fault() {
            Some(ZipSinkFault::Cancelled) => vec!["Aborted".to_string()],
            Some(ZipSinkFault::Io { error, .. }) => vec![format!("{name}: {error}")],
            Some(ZipSinkFault::Caller(error)) => vec![format!("{name}: {error}")],
            Some(ZipSinkFault::Limit { resource, .. }) => {
                vec![format!("{name}: exceeds the {resource:?} limit")]
            }
            _ => vec![format!("{name}: extraction failed ({:?})", error.kind())],
        },
    }
}

/// Methods zipfast's checked reader decodes (stored, deflate, bzip2, LZMA, zstd, xz).
fn zip_method_supported(method: u16) -> bool {
    matches!(method, 0 | 8 | 12 | 14 | 93 | 95)
}

/// WinZip AES members whose ciphertext is over zipfast's 64 KiB in-memory threshold are
/// spooled before their tag is checked; those are the members that need a spool folder and
/// that are read on a helper thread so `is_cancelled` keeps being polled meanwhile.
const ZIP_SPOOLED_CIPHERTEXT: u64 = 64 * 1024;

/// A private (0700) folder inside the destination for zipfast's ciphertext spool. zipfast only
/// spools into a folder it trusts on a local file system; the destination's own file system is
/// disk-backed and is usually one (where /tmp may be overlayfs in a container, or RAM).
/// Created on first use, removed when dropped.
struct ZipSpoolDir {
    parent: PathBuf,
    path: Option<PathBuf>,
}

impl ZipSpoolDir {
    fn new(parent: &Path) -> Self {
        Self {
            parent: parent.to_path_buf(),
            path: None,
        }
    }

    fn get(&mut self) -> Option<PathBuf> {
        if self.path.is_none() {
            let candidate = self.parent.join(format!(
                ".rustyget-zip-spool-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            // create (not create_all): never adopt something that is already there.
            if builder.create(&candidate).is_ok() {
                self.path = Some(candidate);
            }
        }
        self.path.clone()
    }
}

impl Drop for ZipSpoolDir {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// How [`zip_read_member`] decodes a member.
#[derive(Clone, Debug)]
enum ZipReadMode {
    /// zipfast's checked reader; a large AES member is spooled in this folder (or the default
    /// temporary folder for `None`).
    Checked(Option<PathBuf>),
    /// zipfast's in-memory reader (no spool), for an AES member no spool folder accepts.
    /// zipfast limits it to 128 MiB of ciphertext.
    InMemory,
}

/// An AES member up to this size can still be read in memory when zipfast refuses every
/// spool folder (zipfast's own in-memory limit for encrypted members).
const ZIP_IN_MEMORY_LIMIT: u64 = 128 << 20;

/// Decodes one selected member with zipfast into `dest`, returning the bytes written. `poll`
/// runs the read on a helper thread and polls `is_cancelled` here, for members whose spool and
/// authentication pass produce no output for a long time; otherwise the sink polls
/// `is_cancelled` on every chunk. If no helper thread can be started the read runs inline.
#[allow(clippy::too_many_arguments, clippy::result_large_err)]
fn zip_read_member(
    session: &mut zipfast::zip::ReadSession,
    id: &zipfast::zip::EntryId,
    password: Option<&str>,
    how: ZipReadMode,
    cancel: &zipfast::extract::Cancellation,
    dest: &mut std::fs::File,
    poll: bool,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<u64, zipfast::zip::ZipReadError> {
    use std::io::Write;
    let mut overrides = zipfast::zip::EncryptedReadOverrides::default();
    let in_memory = matches!(how, ZipReadMode::InMemory);
    if let ZipReadMode::Checked(temp_dir) = how {
        overrides.temp_dir = temp_dir;
    }
    let read = |session: &mut zipfast::zip::ReadSession,
                dest: &mut std::fs::File,
                check: &mut dyn FnMut() -> bool| {
        let mut written = 0u64;
        if in_memory {
            let selected = session
                .select(id, ZIP_IN_MEMORY_LIMIT, cancel.clone())
                .map_err(|e| {
                    zipfast::zip::ZipReadError::from(zipfast::zip::ReadFault::Session(e))
                })?;
            selected
                .read(password, |chunk| {
                    if check() {
                        cancel.cancel();
                        return Err(zipfast::Error::Io(std::io::ErrorKind::Interrupted.into()));
                    }
                    dest.write_all(chunk).map_err(zipfast::Error::Io)?;
                    written += chunk.len() as u64;
                    Ok(())
                })
                .map_err(|e| {
                    zipfast::zip::ZipReadError::from(zipfast::zip::ReadFault::Session(e))
                })?;
            return Ok(written);
        }
        let mut context = zipfast::zip::EncryptedReadContext::for_session(
            session,
            id,
            None,
            zipfast::zip::ZipReadOutput::Callback,
            password,
            &overrides,
            cancel,
        )?;
        let selected = session
            .select(id, u64::MAX, cancel.clone())
            .map_err(|e| zipfast::zip::ZipReadError::from(zipfast::zip::ReadFault::Session(e)))?;
        selected.read_with_context(password, &mut context, |chunk| {
            if check() {
                cancel.cancel();
                return Err(zipfast::zip::ZipSinkFault::Cancelled);
            }
            dest.write_all(chunk)
                .map_err(|e| zipfast::zip::ZipSinkFault::Caller(zipfast::Error::Io(e)))?;
            written += chunk.len() as u64;
            Ok(())
        })?;
        Ok(written)
    };
    if !poll {
        return read(session, dest, is_cancelled);
    }
    let spawned = std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("zip-member".into())
            .spawn_scoped(scope, || read(&mut *session, &mut *dest, &mut || false))
            .ok()?;
        while !worker.is_finished() {
            if is_cancelled() {
                cancel.cancel();
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Some(worker.join().unwrap_or_else(|_| {
            Err(
                zipfast::zip::ReadFault::Legacy(zipfast::Error::Io(std::io::Error::other(
                    "the zipfast reader stopped unexpectedly",
                )))
                .into(),
            )
        }))
    });
    match spawned {
        Some(result) => result,
        None => read(session, dest, is_cancelled),
    }
}

/// Extracts a ZIP with zipfast doing all decoding and verification: one `ReadSession` parses
/// the central directory once, and every member is decoded by zipfast's checked reader
/// straight into a destination file RustyGet opened itself (no-follow, Skip / Overwrite /
/// AutoRename), so the output works on any file system and keeps RustyGet's umask, mode and
/// mtime rules. A member that would be refused (no password, unsupported method) is refused
/// before its destination is opened. Output of a member that fails is removed when this run
/// created the file, and the first failure stops the run.
fn extract_zip_cancellable<F>(
    archive_path: &Path,
    output_dir: &Path,
    password: &str,
    overwrite_mode: i32,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    use std::io::{Seek, SeekFrom};

    // The central directory is read first so links and unreadable names refuse the whole
    // archive before anything is written.
    let archive = zipfast::zip::Archive::open(archive_path).map_err(|e| {
        let msg = e.to_string();
        log_cb(1, &msg);
        msg
    })?;
    if archive.entries.iter().any(|e| e.is_link()) {
        log_cb(1, "ERROR: Dangerous link path was ignored");
        log_cb(0, "Stream repair: archive contains link");
        return Err("archive contains link".to_string());
    }
    if let Some(e) = archive.entries.iter().find(|e| e.name.contains('\u{FFFD}')) {
        let msg = format!("{}: invalid UTF-8 filename", e.name);
        log_cb(1, &msg);
        return Err(msg);
    }
    drop(archive);

    std::fs::create_dir_all(output_dir).map_err(|e| {
        let msg = format!("Could not create {}: {e}", output_dir.display());
        log_cb(1, &msg);
        msg
    })?;

    let cancel = zipfast::extract::Cancellation::default();
    let open_session = |cancel: &zipfast::extract::Cancellation| {
        zipfast::zip::ReadSession::open_cancelable(archive_path, cancel)
            .map_err(|e| format!("{}: {e}", archive_path.display()))
    };
    let mut session = open_session(&cancel).inspect_err(|msg| log_cb(1, msg))?;
    let mut metadata = session.metadata();
    if let Some(vol_name) = archive_path.file_name().and_then(|s| s.to_str()) {
        log_cb(4, &format!("Unrar: Extracting from {vol_name}"));
    }

    let pw = (!password.is_empty()).then_some(password);
    let mut spool = ZipSpoolDir::new(output_dir);
    let mut summary = zipfast::extract::Summary::default();
    summary.volumes = 1;

    for ordinal in 0..metadata.len() {
        if is_cancelled() {
            return Err("Aborted".to_string());
        }
        let Some(mut id) = metadata.entry_id(ordinal) else {
            let msg = format!("{}: entry {ordinal} is missing", archive_path.display());
            log_cb(1, &msg);
            return Err(msg);
        };
        let entry = metadata.entry(&id).map_err(|e| {
            let msg = format!("{}: {e}", archive_path.display());
            log_cb(1, &msg);
            msg
        })?;
        let name = entry.name().to_string();
        summary.matched += 1;
        let rel = zipfast::path::sanitize(&name, entry.creator_host() != 3).map_err(|err| {
            let msg = format!("{name}: {err}");
            log_cb(1, &msg);
            msg
        })?;
        if entry.is_dir() {
            create_dir_all_no_follow(output_dir, &rel).inspect_err(|err| log_cb(1, err))?;
            summary.dirs += 1;
            continue;
        }
        // Refusals that need no decoding happen before the destination is opened, so an
        // existing file is never emptied for a member that cannot be extracted.
        if entry.encrypted() && pw.is_none() {
            log_cb(1, ZIP_BAD_PASSWORD);
            let msg = format!("{name}: wrong password");
            log_cb(1, &msg);
            return Err(msg);
        }
        if !zip_method_supported(entry.effective_method()) {
            let msg = format!(
                "{name}: compression method {} is not supported",
                entry.effective_method()
            );
            log_cb(1, &msg);
            return Err(msg);
        }
        let mode = entry.extraction_unix_mode();
        let mtime = entry.modified();
        // Only WinZip AES (raw method 99) is spooled; ZipCrypto streams.
        let aes = entry.encrypted() && entry.listing_method() == 99;
        let spooled = aes && entry.packed_size() > ZIP_SPOOLED_CIPHERTEXT;
        let packed = entry.packed_size();

        let Some((mut dest_file, target_path, created_new)) =
            create_dest_file_with_path_no_follow(output_dir, &rel, overwrite_mode)
                .inspect_err(|err| log_cb(1, err))?
        else {
            summary.skipped += 1;
            continue;
        };
        log_cb(4, &format!("Unrar: Extracting  {name}"));

        // Ways to read it, in order: for a spooled member a private spool folder beside the
        // output, then the default temporary folder, then (up to 128 MiB) in memory.
        let mut ways: Vec<ZipReadMode> = Vec::new();
        if spooled {
            if let Some(dir) = spool.get() {
                ways.push(ZipReadMode::Checked(Some(dir)));
            }
        }
        ways.push(ZipReadMode::Checked(None));
        if spooled && packed <= ZIP_IN_MEMORY_LIMIT {
            ways.push(ZipReadMode::InMemory);
        }
        // Reopens allowed for this member when something touches the archive mid-read.
        let mut reopens_left = 3u32;
        let mut attempt = 0usize;
        let res = loop {
            let res = zip_read_member(
                &mut session,
                &id,
                pw,
                ways[attempt].clone(),
                &cancel,
                &mut dest_file,
                spooled,
                is_cancelled,
            );
            let Err(err) = &res else {
                break res;
            };
            if zip_error_is_cancel(err) || cancel.is_canceled() {
                break res;
            }
            let storage = matches!(
                err.operation(),
                Some(zipfast::zip::ReadFault::UnsupportedStorage)
                    | Some(zipfast::zip::ReadFault::UnsupportedIdentity)
            );
            let source_changed = err.is_source_changed()
                || matches!(
                    err.operation(),
                    Some(zipfast::zip::ReadFault::Session(
                        zipfast::zip::SessionError::SourceChanged { .. }
                    ))
                );
            if storage && attempt + 1 < ways.len() {
                attempt += 1;
            } else if source_changed && reopens_left > 0 {
                // Something touched the archive (chmod, xattr, indexer): read it again from a
                // fresh session, if it still lists the same members.
                reopens_left -= 1;
                let Ok(fresh) = open_session(&cancel) else {
                    break res;
                };
                let fresh_meta = fresh.metadata();
                if fresh_meta.len() != metadata.len() {
                    break res;
                }
                let same = fresh_meta
                    .entry_id(ordinal)
                    .and_then(|fid| fresh_meta.entry(&fid).ok().map(|e| (fid, e.name() == name)));
                match same {
                    Some((fid, true)) => {
                        session = fresh;
                        metadata = fresh_meta;
                        id = fid;
                    }
                    _ => break res,
                }
            } else {
                break res;
            }
            if dest_file.set_len(0).is_err() || dest_file.seek(SeekFrom::Start(0)).is_err() {
                break res;
            }
        };

        let written = match res {
            Ok(written) => written,
            Err(err) => {
                drop(dest_file);
                if created_new {
                    let _ = std::fs::remove_file(&target_path);
                }
                if zip_error_is_cancel(&err) || cancel.is_canceled() {
                    return Err("Aborted".to_string());
                }
                let lines = zip_member_failure_lines(&name, &err);
                for line in &lines {
                    log_cb(1, line);
                }
                return Err(lines.last().cloned().unwrap_or_default());
            }
        };

        if let Some(mode) = mode {
            apply_unix_executable_mode(&dest_file, mode);
        }
        if let Some((secs, nanos)) = mtime {
            set_file_mtime(&dest_file, Some(secs), nanos);
        }
        summary.files += 1;
        summary.bytes += written;
    }

    Ok(summary)
}

fn tar_octal(field: &[u8]) -> Option<u64> {
    let mut digits = field
        .iter()
        .skip_while(|&&b| b == b' ')
        .take_while(|&&b| b != 0 && b != b' ')
        .peekable();
    digits.peek()?;
    let mut v = 0u64;
    for &d in digits {
        if !(b'0'..=b'7').contains(&d) {
            return None;
        }
        v = v.checked_mul(8)?.checked_add(u64::from(d - b'0'))?;
    }
    Some(v)
}

fn tar_number(field: &[u8]) -> Option<i64> {
    match field.first() {
        Some(&b) if b & 0x80 != 0 => {
            let mut v = i64::from(b & 0x7f);
            if b & 0x40 != 0 {
                v -= 0x80;
            }
            for &b in &field[1..] {
                v = v.checked_mul(256)?.checked_add(i64::from(b))?;
            }
            Some(v)
        }
        _ => tar_octal(field).and_then(|v| i64::try_from(v).ok()),
    }
}

fn tar_checksum_ok(h: &[u8]) -> bool {
    if h.len() < 512 || h[..512].iter().all(|&b| b == 0) {
        return false;
    }
    let Some(stored) = tar_octal(&h[148..156]) else {
        return false;
    };
    let (mut unsigned, mut signed) = (0u64, 0i64);
    for (i, &b) in h[..512].iter().enumerate() {
        let b = if (148..156).contains(&i) { b' ' } else { b };
        unsigned += u64::from(b);
        signed += i64::from(b as i8);
    }
    stored == unsigned || stored as i64 == signed
}

fn tar_text(field: &[u8]) -> &[u8] {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    &field[..end]
}

fn sanitize_tar_rel_path_bytes(raw_name: &[u8]) -> Result<PathBuf, String> {
    let raw_name = tar_text(raw_name);
    let mut out = PathBuf::new();
    for seg in raw_name.split(|&b| b == b'/' || b == b'\\') {
        if seg.is_empty() || seg == b"." {
            continue;
        }
        if seg == b".." || seg.contains(&0) {
            return Err(format!(
                "Unsafe tar member path: {}",
                String::from_utf8_lossy(raw_name)
            ));
        }
        // Preserve raw path bytes on POSIX platforms whose VFS accepts arbitrary byte
        // sequences. On Darwin/APFS (`target_vendor = "apple"`) and non-Unix targets where
        // the kernel rejects non-UTF-8 path components with `EILSEQ`, use an injective
        // byte-to-UTF-8 encoding so distinct legacy byte sequences never collide.
        #[cfg(all(unix, not(target_vendor = "apple")))]
        {
            use std::os::unix::ffi::OsStrExt;
            out.push(std::ffi::OsStr::from_bytes(seg));
        }
        #[cfg(any(not(unix), target_vendor = "apple"))]
        {
            out.push(encode_bytes_injective_utf8(seg));
        }
    }
    if out.as_os_str().is_empty() {
        return Err("Empty tar member path".to_string());
    }
    Ok(out)
}

fn tar_read_full(r: &mut dyn Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

fn tar_single_name(codec: zipfast::stream::Codec, path: &Path) -> String {
    let clean = |name: &str| {
        let last = name
            .rsplit(['/', '\\'])
            .find(|s| !s.is_empty() && *s != "." && *s != "..")?;
        let rel = zipfast::path::sanitize(last, true).ok()?;
        Some(rel.file_name()?.to_string_lossy().into_owned())
    };
    if codec == zipfast::stream::Codec::Gzip {
        if let Some(name) = zipfast::stream::gzip_name(path).as_deref().and_then(clean) {
            return name;
        }
    }
    let file = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lower = file.to_ascii_lowercase();
    for (ending, becomes) in codec.endings() {
        if lower.ends_with(ending) && lower.len() > ending.len() {
            let stem = format!("{}{becomes}", &file[..file.len() - ending.len()]);
            return clean(&stem).unwrap_or_else(|| "file".to_string());
        }
    }
    clean(&format!("{file}.out")).unwrap_or_else(|| "file".to_string())
}

fn extract_tar_or_stream_cancellable<F>(
    archive_path: &Path,
    output_dir: &Path,
    overwrite_mode: i32,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    if is_cancelled() {
        return Err("Aborted".to_string());
    }
    let mut file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let mut head6 = [0u8; 6];
    let n6 = tar_read_full(&mut file, &mut head6).map_err(|e| e.to_string())?;
    file.rewind().map_err(|e| e.to_string())?;
    let codec = zipfast::stream::Codec::sniff(&head6[..n6]);
    let buf_reader = std::io::BufReader::with_capacity(256 * 1024, file);
    let mut stream: Box<dyn Read> = match codec {
        Some(c) => c.open(buf_reader, 3).map_err(|e| e.to_string())?,
        None => Box::new(buf_reader),
    };

    if let Some(vol_name) = archive_path.file_name().and_then(|s| s.to_str()) {
        log_cb(4, &format!("Unrar: Extracting from {vol_name}"));
    }

    let mut first = vec![0u8; 512];
    let first_n = tar_read_full(&mut *stream, &mut first).map_err(|e| e.to_string())?;
    first.truncate(first_n);

    let mut is_tar = first_n == 512 && tar_checksum_ok(&first);
    if first_n == 512 && first.iter().all(|&b| b == 0) {
        first.resize(1024, 0);
        let more = tar_read_full(&mut *stream, &mut first[512..]).map_err(|e| e.to_string())?;
        first.truncate(512 + more);
        if first.len() == 1024 && first.iter().all(|&b| b == 0) {
            is_tar = true;
            first.clear();
        }
    }

    let mut summary = zipfast::extract::Summary::default();
    summary.volumes = 1;
    let mut io_buf = vec![0u8; 64 * 1024];

    if !is_tar {
        let Some(c) = codec else {
            return Err("Not a valid tar or compressed stream archive".to_string());
        };
        let name = tar_single_name(c, archive_path);
        let rel = PathBuf::from(&name);
        let opened = create_dest_file_with_path_no_follow(output_dir, &rel, overwrite_mode)?;
        let Some((mut dest_file, target_path, created_new)) = opened else {
            summary.skipped += 1;
            return Ok(summary);
        };
        log_cb(4, &format!("Unrar: Extracting  {name}"));
        let write_res = (|| -> Result<u64, String> {
            let mut total = 0u64;
            if !first.is_empty() {
                if is_cancelled() {
                    return Err("Aborted".to_string());
                }
                dest_file.write_all(&first).map_err(|e| e.to_string())?;
                total += first.len() as u64;
            }
            loop {
                if is_cancelled() {
                    return Err("Aborted".to_string());
                }
                let got = tar_read_full(&mut *stream, &mut io_buf).map_err(|e| e.to_string())?;
                if got == 0 {
                    break;
                }
                if is_cancelled() {
                    return Err("Aborted".to_string());
                }
                dest_file
                    .write_all(&io_buf[..got])
                    .map_err(|e| e.to_string())?;
                total += got as u64;
            }
            Ok(total)
        })();
        match write_res {
            Ok(total) => {
                summary.files += 1;
                summary.bytes += total;
                return Ok(summary);
            }
            Err(err) => {
                drop(dest_file);
                if created_new {
                    let _ = std::fs::remove_file(&target_path);
                }
                if err != "Aborted" {
                    log_cb(1, &err);
                }
                return Err(err);
            }
        }
    }

    let mut pending_first = (!first.is_empty()).then_some(first);
    let mut long_name: Option<Vec<u8>> = None;
    let mut pax_path: Option<Vec<u8>> = None;
    let mut pax_size: Option<u64> = None;
    let mut pax_sparse = false;
    let mut global_sparse = false;

    let read_next_block = |stream: &mut dyn Read,
                           pending_first: &mut Option<Vec<u8>>|
     -> Result<Option<[u8; 512]>, String> {
        if let Some(f) = pending_first.take() {
            return Ok(f.as_slice().try_into().ok());
        }
        let mut block = [0u8; 512];
        let got = tar_read_full(stream, &mut block).map_err(|e| e.to_string())?;
        if got == 0 || (got == 512 && block.iter().all(|&b| b == 0)) {
            return Ok(None);
        }
        if got < 512 {
            return Err("Truncated tar header".to_string());
        }
        if !tar_checksum_ok(&block) {
            return Err("Corrupt tar header checksum".to_string());
        }
        Ok(Some(block))
    };

    let skip_bytes = |stream: &mut dyn Read,
                      mut left: u64,
                      buf: &mut [u8],
                      is_cancelled: &mut dyn FnMut() -> bool|
     -> Result<(), String> {
        while left > 0 {
            if is_cancelled() {
                return Err("Aborted".to_string());
            }
            let want = (left as usize).min(buf.len());
            let got = tar_read_full(stream, &mut buf[..want]).map_err(|e| e.to_string())?;
            if got < want {
                return Err("Truncated tar entry".to_string());
            }
            left -= got as u64;
        }
        Ok(())
    };

    loop {
        if is_cancelled() {
            return Err("Aborted".to_string());
        }
        let Some(h) = read_next_block(&mut *stream, &mut pending_first)? else {
            if codec.is_some() {
                loop {
                    if is_cancelled() {
                        return Err("Aborted".to_string());
                    }
                    let got = tar_read_full(&mut *stream, &mut io_buf).map_err(|e| {
                        let msg = format!("Corrupt compressed tar trailer: {e}");
                        log_cb(1, &msg);
                        msg
                    })?;
                    if got == 0 {
                        break;
                    }
                }
            }
            break;
        };
        let kind = h[156];
        let raw_size = tar_number(&h[124..136])
            .and_then(|v| u64::try_from(v).ok())
            .ok_or_else(|| "Unreadable tar header size".to_string())?;
        let pad = (512 - (raw_size % 512)) % 512;

        if matches!(kind, b'L' | b'K' | b'x' | b'X' | b'g') {
            if raw_size > (1 << 20) {
                return Err("Tar extension header too large".to_string());
            }
            let mut ext = vec![0u8; raw_size as usize];
            if tar_read_full(&mut *stream, &mut ext).map_err(|e| e.to_string())? < ext.len() {
                return Err("Truncated tar extension header".to_string());
            }
            skip_bytes(&mut *stream, pad, &mut io_buf, is_cancelled)?;
            if kind == b'L' {
                long_name = Some(tar_text(&ext).to_vec());
            } else if matches!(kind, b'x' | b'X' | b'g') {
                let mut rest = ext.as_slice();
                while !rest.is_empty() && !rest.iter().all(|&b| b == 0) {
                    let Some(sp) = rest.iter().position(|&b| b == b' ') else {
                        return Err("Corrupt PAX extension header".to_string());
                    };
                    let Some(len) = std::str::from_utf8(&rest[..sp])
                        .ok()
                        .and_then(|s| s.parse::<usize>().ok())
                    else {
                        return Err("Corrupt PAX extension header".to_string());
                    };
                    if len <= sp + 1 || len > rest.len() || rest[len - 1] != b'\n' {
                        return Err("Corrupt PAX extension header".to_string());
                    }
                    let rec = &rest[sp + 1..len - 1];
                    rest = &rest[len..];
                    let Some(eq) = rec.iter().position(|&b| b == b'=') else {
                        return Err("Corrupt PAX extension header".to_string());
                    };
                    let (k, v) = (&rec[..eq], &rec[eq + 1..]);
                    if kind != b'g' && k == b"path" {
                        pax_path = Some(v.to_vec());
                    } else if kind != b'g' && k == b"size" {
                        pax_size = std::str::from_utf8(v).ok().and_then(|s| s.parse().ok());
                    } else if k.starts_with(b"GNU.sparse.") {
                        if kind == b'g' {
                            global_sparse = true;
                        } else {
                            pax_sparse = true;
                        }
                    }
                }
            }
            continue;
        }

        let ustar = &h[257..263] == b"ustar\0";
        let mut raw_name = tar_text(&h[0..100]).to_vec();
        if ustar {
            let prefix = tar_text(&h[345..500]);
            if !prefix.is_empty() {
                raw_name = [prefix, b"/", &raw_name].concat();
            }
        }
        let name_bytes = pax_path
            .take()
            .or_else(|| long_name.take())
            .unwrap_or(raw_name);
        let name = String::from_utf8_lossy(&name_bytes).into_owned();
        let size = pax_size.take().unwrap_or(raw_size);
        let is_sparse = std::mem::replace(&mut pax_sparse, false) || global_sparse || kind == b'S';
        let entry_pad = (512 - (size % 512)) % 512;
        let mode = tar_number(&h[100..108]).unwrap_or(0o644) as u32 & 0o7777;
        let mtime = tar_number(&h[136..148]);

        if kind == b'V' {
            continue;
        }
        if matches!(kind, b'1' | b'2') {
            log_cb(1, "ERROR: Dangerous link path was ignored");
            log_cb(0, "Stream repair: archive contains link");
            return Err("archive contains link".to_string());
        }
        if is_sparse {
            let msg = format!("{name}: sparse files are not supported");
            log_cb(1, &msg);
            return Err(msg);
        }
        if kind == b'M' {
            let msg = format!("{name}: multi-volume tar archives are not supported");
            log_cb(1, &msg);
            return Err(msg);
        }
        if matches!(kind, b'3' | b'4' | b'6') {
            let msg = format!("{name}: devices and pipes are not extracted");
            log_cb(1, &msg);
            return Err(msg);
        }

        let is_dir = matches!(kind, b'5' | b'D')
            || (matches!(kind, b'0' | 0 | b'7') && name_bytes.ends_with(b"/"));
        if is_dir
            && name_bytes
                .split(|&b| b == b'/' || b == b'\\')
                .all(|p| p.is_empty() || p == b".")
        {
            continue;
        }
        let rel = sanitize_tar_rel_path_bytes(&name_bytes)?;
        if is_dir {
            create_dir_all_no_follow(output_dir, &rel)?;
            summary.dirs += 1;
            continue;
        }

        let opened = create_dest_file_with_path_no_follow(output_dir, &rel, overwrite_mode)?;
        let Some((mut dest_file, target_path, created_new)) = opened else {
            summary.skipped += 1;
            skip_bytes(&mut *stream, size + entry_pad, &mut io_buf, is_cancelled)?;
            continue;
        };

        log_cb(4, &format!("Unrar: Extracting  {name}"));
        let copy_res = (|| -> Result<(), String> {
            let mut left = size;
            while left > 0 {
                if is_cancelled() {
                    return Err("Aborted".to_string());
                }
                let want = (left as usize).min(io_buf.len());
                let got =
                    tar_read_full(&mut *stream, &mut io_buf[..want]).map_err(|e| e.to_string())?;
                if got < want {
                    return Err(format!("{name}: truncated tar entry"));
                }
                if is_cancelled() {
                    return Err("Aborted".to_string());
                }
                dest_file
                    .write_all(&io_buf[..got])
                    .map_err(|e| e.to_string())?;
                left -= got as u64;
            }
            skip_bytes(&mut *stream, entry_pad, &mut io_buf, is_cancelled)?;
            Ok(())
        })();

        if let Err(err) = copy_res {
            drop(dest_file);
            if created_new {
                let _ = std::fs::remove_file(&target_path);
            }
            if err != "Aborted" {
                log_cb(1, &err);
            }
            return Err(err);
        }

        apply_unix_executable_mode(&dest_file, mode);
        set_file_mtime(&dest_file, mtime, 0);
        summary.files += 1;
        summary.bytes += size;
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempTestDir(PathBuf);

    impl TempTestDir {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "rustyget_archive_native_test_{}_{}_{}",
                tag,
                std::process::id(),
                id
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_sha256_state_incremental_matches_oneshot() {
        let salt = b"01234567";
        let pw_utf16 = b"s\0e\0c\0r\0e\0t\0";
        let rounds = 1u64 << 10;
        let mut concat = Vec::new();
        let mut inc = Sha256State::new();
        for r in 0..rounds {
            concat.extend_from_slice(salt);
            concat.extend_from_slice(pw_utf16);
            concat.extend_from_slice(&r.to_le_bytes());

            inc.update(salt);
            inc.update(pw_utf16);
            inc.update(&r.to_le_bytes());
        }
        assert_eq!(inc.finalize(), zipfast::crypto::sha256(&concat));
    }

    fn build_single_entry_zip(
        name: &[u8],
        flags: u16,
        method: u16,
        crc32_val: u32,
        unpacked_size: u32,
        extra: &[u8],
        payload: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        // Local file header
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc32_val.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&unpacked_size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(extra);
        out.extend_from_slice(payload);

        let cd_offset = out.len() as u32;
        // Central directory header
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc32_val.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&unpacked_size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(extra);
        let cd_size = (out.len() as u32) - cd_offset;

        // End of central directory
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn test_zip_streaming_winzip_aes_and_zipcrypto_and_corrupt_auth() {
        let plain = b"streaming encrypted zip payload verification test!\n";
        let plain_crc = crc32(plain);
        let password = "correct-horse-battery-staple";

        // 1. Build a WinZip AES-256 (strength=3, AE-2 vendor=2, actual_method=0 stored) ZIP entry
        let salt = [0x11u8; 16];
        let key_len = 32usize;
        let derived = zipfast::crypto::aes_derive(password.as_bytes(), &salt, key_len).unwrap();
        let verifier = &derived[key_len * 2..key_len * 2 + 2];
        let ct = zipfast::crypto::decrypt_winzip_ctr(&derived[..key_len], plain).unwrap();
        let auth = {
            let key = openssl::pkey::PKey::hmac(&derived[key_len..key_len * 2]).unwrap();
            let mut signer =
                openssl::sign::Signer::new(openssl::hash::MessageDigest::sha1(), &key).unwrap();
            signer.update(&ct).unwrap();
            signer.sign_to_vec().unwrap()
        };

        let mut aes_payload = Vec::new();
        aes_payload.extend_from_slice(&salt);
        aes_payload.extend_from_slice(verifier);
        aes_payload.extend_from_slice(&ct);
        aes_payload.extend_from_slice(&auth[..10]);

        let mut aes_extra = Vec::new();
        aes_extra.extend_from_slice(&0x9901u16.to_le_bytes());
        aes_extra.extend_from_slice(&7u16.to_le_bytes());
        aes_extra.extend_from_slice(&2u16.to_le_bytes()); // AE-2
        aes_extra.extend_from_slice(b"AE");
        aes_extra.push(3); // AES-256
        aes_extra.extend_from_slice(&0u16.to_le_bytes()); // inner method 0 (stored)

        let zip_bytes = build_single_entry_zip(
            b"aes.txt",
            0x0001,
            99,
            0, // AE-2 sets CRC to 0
            plain.len() as u32,
            &aes_extra,
            &aes_payload,
        );

        let dir = TempTestDir::new("aes_zc");
        let zip_path = dir.path().join("aes.zip");
        let out_dir = dir.path().join("out_aes");
        std::fs::write(&zip_path, &zip_bytes).unwrap();

        let mut logs = Vec::new();
        let summary = extract_zip_cancellable(
            &zip_path,
            &out_dir,
            password,
            0,
            &mut |k, m| logs.push((k, m.to_string())),
            &mut || false,
        )
        .unwrap();
        assert_eq!(summary.files, 1);
        assert_eq!(std::fs::read(out_dir.join("aes.txt")).unwrap(), plain);

        // Overwrite mode never empties an existing file for a member that is refused before
        // decoding (no password here, an unsupported method below).
        {
            let out = dir.path().join("out_aes_keep");
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join("aes.txt"), b"keep me").unwrap();
            assert!(
                extract_zip_cancellable(&zip_path, &out, "", 1, &mut |_, _| {}, &mut || false)
                    .is_err()
            );
            assert_eq!(std::fs::read(out.join("aes.txt")).unwrap(), b"keep me");

            let ppmd = build_single_entry_zip(b"aes.txt", 0, 98, 0, 4, &[], b"xxxx");
            let ppmd_path = dir.path().join("ppmd.zip");
            std::fs::write(&ppmd_path, &ppmd).unwrap();
            let err =
                extract_zip_cancellable(&ppmd_path, &out, "", 1, &mut |_, _| {}, &mut || false)
                    .unwrap_err();
            assert!(err.contains("compression method 98"), "{err}");
            assert_eq!(std::fs::read(out.join("aes.txt")).unwrap(), b"keep me");
        }

        // A missing or wrong password logs the line unpack.rs keys on, in every mode, and
        // leaves no file behind.
        for (mode, pw) in [(0, ""), (1, "wrong"), (2, "wrong")] {
            let out = dir.path().join(format!("out_aes_badpw{mode}"));
            let mut logs = Vec::new();
            let err = extract_zip_cancellable(
                &zip_path,
                &out,
                pw,
                mode,
                &mut |_, m| logs.push(m.to_string()),
                &mut || false,
            )
            .unwrap_err();
            assert!(err.contains("wrong password"), "mode {mode}: {err}");
            assert!(
                logs.iter()
                    .any(|m| m == "Unrar: The specified password is incorrect."),
                "mode {mode}: {logs:?}"
            );
            assert!(!out.join("aes.txt").exists(), "mode {mode}");
        }

        // Corrupt the 10-byte HMAC-SHA1 tag at the end of the WinZip AES payload
        let mut corrupt_aes_payload = aes_payload.clone();
        *corrupt_aes_payload.last_mut().unwrap() ^= 0xff;
        let bad_zip_bytes = build_single_entry_zip(
            b"aes.txt",
            0x0001,
            99,
            0,
            plain.len() as u32,
            &aes_extra,
            &corrupt_aes_payload,
        );
        let bad_zip_path = dir.path().join("aes_bad.zip");
        let bad_out_dir = dir.path().join("out_aes_bad");
        std::fs::write(&bad_zip_path, &bad_zip_bytes).unwrap();
        let err = extract_zip_cancellable(
            &bad_zip_path,
            &bad_out_dir,
            password,
            0,
            &mut |_, _| {},
            &mut || false,
        )
        .unwrap_err();
        assert!(err.contains("CRC failed"), "unexpected error: {err}");

        // 2. Build a ZipCrypto (method 0 stored) ZIP entry
        // ZipCrypto key schedule (APPNOTE 6.1.5), here only to build the fixture.
        fn zipcrypto_update(keys: &mut [u32; 3], b: u8) {
            let crc_byte = |crc: u32, b: u8| {
                let mut c = (crc ^ u32::from(b)) & 0xff;
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        (c >> 1) ^ 0xEDB8_8320
                    } else {
                        c >> 1
                    };
                }
                c ^ (crc >> 8)
            };
            keys[0] = crc_byte(keys[0], b);
            keys[1] = keys[1]
                .wrapping_add(keys[0] & 0xff)
                .wrapping_mul(134_775_813)
                .wrapping_add(1);
            keys[2] = crc_byte(keys[2], (keys[1] >> 24) as u8);
        }
        let mut keys = [0x1234_5678u32, 0x2345_6789, 0x3456_7890];
        for &b in password.as_bytes() {
            zipcrypto_update(&mut keys, b);
        }
        let mut zc_payload = Vec::new();
        let mut hdr_plain = [0x42u8; 12];
        hdr_plain[11] = (plain_crc >> 24) as u8;
        for &p in hdr_plain.iter().chain(plain.iter()) {
            let t = keys[2] | 2;
            let k = (t.wrapping_mul(t ^ 1) >> 8) as u8;
            zc_payload.push(p ^ k);
            zipcrypto_update(&mut keys, p);
        }
        let zc_zip_bytes = build_single_entry_zip(
            b"zc.txt",
            0x0001,
            0,
            plain_crc,
            plain.len() as u32,
            &[],
            &zc_payload,
        );
        let zc_zip_path = dir.path().join("zc.zip");
        let zc_out_dir = dir.path().join("out_zc");
        std::fs::write(&zc_zip_path, &zc_zip_bytes).unwrap();
        let zc_summary = extract_zip_cancellable(
            &zc_zip_path,
            &zc_out_dir,
            password,
            0,
            &mut |_, _| {},
            &mut || false,
        )
        .unwrap();
        assert_eq!(zc_summary.files, 1);
        assert_eq!(std::fs::read(zc_out_dir.join("zc.txt")).unwrap(), plain);
    }

    #[test]
    fn test_zip_overwrite_modes_skip_always_rename() {
        let dir = TempTestDir::new("zip_modes");
        let zip_path = dir.path().join("modes.zip");
        let out = dir.path().join("out");
        let payload = b"fresh payload";
        std::fs::write(
            &zip_path,
            build_single_entry_zip(
                b"a.txt",
                0,
                0,
                crc32(payload),
                payload.len() as u32,
                &[],
                payload,
            ),
        )
        .unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let run = |mode: i32| {
            let mut logs = Vec::new();
            let summary = extract_zip_cancellable(
                &zip_path,
                &out,
                "",
                mode,
                &mut |_, m| logs.push(m.to_string()),
                &mut || false,
            )
            .unwrap();
            (summary, logs)
        };

        std::fs::write(out.join("a.txt"), b"old").unwrap();
        let (skip, _) = run(0);
        assert_eq!((skip.files, skip.skipped), (0, 1));
        assert_eq!(std::fs::read(out.join("a.txt")).unwrap(), b"old");

        let (always, logs) = run(1);
        assert_eq!(always.files, 1);
        assert_eq!(std::fs::read(out.join("a.txt")).unwrap(), payload);
        assert!(
            logs.iter().any(|m| m == "Unrar: Extracting  a.txt"),
            "{logs:?}"
        );

        let (renamed, _) = run(2);
        assert_eq!(renamed.files, 1);
        assert_eq!(std::fs::read(out.join("a.txt")).unwrap(), payload);
        assert_eq!(std::fs::read(out.join("a_1.txt")).unwrap(), payload);
        let leftovers: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".rustyget-zip-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging folder left behind: {leftovers:?}"
        );
    }

    #[test]
    fn test_zip_large_winzip_aes_member_leaves_nothing_behind() {
        // Over zipfast's 64 KiB in-memory threshold, so the ciphertext is spooled (in the
        // temporary folder) before the tag is checked; only the member reaches the output.
        let plain: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let password = "pw";
        let salt = [0x22u8; 16];
        let key_len = 32usize;
        let derived = zipfast::crypto::aes_derive(password.as_bytes(), &salt, key_len).unwrap();
        let ct = zipfast::crypto::decrypt_winzip_ctr(&derived[..key_len], &plain).unwrap();
        let auth = {
            let key = openssl::pkey::PKey::hmac(&derived[key_len..key_len * 2]).unwrap();
            let mut signer =
                openssl::sign::Signer::new(openssl::hash::MessageDigest::sha1(), &key).unwrap();
            signer.update(&ct).unwrap();
            signer.sign_to_vec().unwrap()
        };
        let mut payload = Vec::new();
        payload.extend_from_slice(&salt);
        payload.extend_from_slice(&derived[key_len * 2..key_len * 2 + 2]);
        payload.extend_from_slice(&ct);
        payload.extend_from_slice(&auth[..10]);
        let mut extra = Vec::new();
        extra.extend_from_slice(&0x9901u16.to_le_bytes());
        extra.extend_from_slice(&7u16.to_le_bytes());
        extra.extend_from_slice(&2u16.to_le_bytes());
        extra.extend_from_slice(b"AE");
        extra.push(3);
        extra.extend_from_slice(&0u16.to_le_bytes());
        let zip =
            build_single_entry_zip(b"big.bin", 1, 99, 0, plain.len() as u32, &extra, &payload);

        let dir = TempTestDir::new("aes_big");
        let zip_path = dir.path().join("big.zip");
        std::fs::write(&zip_path, &zip).unwrap();
        for mode in [1, 2] {
            let out = dir.path().join(format!("out{mode}"));
            // A shared (world-writable) output folder: zipfast does not trust it for its
            // spool, so the private folder RustyGet makes inside it is what gets used.
            std::fs::create_dir_all(&out).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o777)).unwrap();
            }
            let summary = extract_zip_cancellable(
                &zip_path,
                &out,
                password,
                mode,
                &mut |_, _| {},
                &mut || false,
            )
            .unwrap();
            assert_eq!(summary.files, 1, "mode {mode}");
            assert_eq!(
                std::fs::read(out.join("big.bin")).unwrap(),
                plain,
                "mode {mode}"
            );
            let names: Vec<_> = std::fs::read_dir(&out)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(names, vec!["big.bin".to_string()], "mode {mode}");
        }
    }

    #[test]
    fn test_zip_cancel_is_seen_while_a_large_aes_member_spools() {
        // While zipfast spools and authenticates a large AES member nothing reaches the
        // output, so cancellation has to be polled beside the read, not in the output sink.
        let plain = vec![0x33u8; 48 << 20];
        let password = "pw";
        let salt = [0x44u8; 16];
        let key_len = 32usize;
        let derived = zipfast::crypto::aes_derive(password.as_bytes(), &salt, key_len).unwrap();
        let ct = zipfast::crypto::decrypt_winzip_ctr(&derived[..key_len], &plain).unwrap();
        let auth = {
            let key = openssl::pkey::PKey::hmac(&derived[key_len..key_len * 2]).unwrap();
            let mut signer =
                openssl::sign::Signer::new(openssl::hash::MessageDigest::sha1(), &key).unwrap();
            signer.update(&ct).unwrap();
            signer.sign_to_vec().unwrap()
        };
        let mut payload = Vec::with_capacity(ct.len() + 28);
        payload.extend_from_slice(&salt);
        payload.extend_from_slice(&derived[key_len * 2..key_len * 2 + 2]);
        payload.extend_from_slice(&ct);
        payload.extend_from_slice(&auth[..10]);
        drop(ct);
        let mut extra = Vec::new();
        extra.extend_from_slice(&0x9901u16.to_le_bytes());
        extra.extend_from_slice(&7u16.to_le_bytes());
        extra.extend_from_slice(&2u16.to_le_bytes());
        extra.extend_from_slice(b"AE");
        extra.push(3);
        extra.extend_from_slice(&0u16.to_le_bytes());
        let zip =
            build_single_entry_zip(b"big.bin", 1, 99, 0, plain.len() as u32, &extra, &payload);
        drop(payload);
        let dir = TempTestDir::new("aes_cancel");
        let zip_path = dir.path().join("big.zip");
        std::fs::write(&zip_path, &zip).unwrap();
        drop(zip);

        let started = std::time::Instant::now();
        extract_zip_cancellable(
            &zip_path,
            &dir.path().join("full"),
            password,
            1,
            &mut |_, _| {},
            &mut || false,
        )
        .unwrap();
        let full = started.elapsed();

        let out = dir.path().join("cancelled");
        let run_started = std::cell::Cell::new(false);
        let mut polls = 0u32;
        let started = std::time::Instant::now();
        let err = extract_zip_cancellable(
            &zip_path,
            &out,
            password,
            1,
            &mut |_, m| {
                if m == "Unrar: Extracting  big.bin" {
                    run_started.set(true);
                }
            },
            &mut || {
                if run_started.get() {
                    polls += 1;
                }
                polls > 2
            },
        )
        .unwrap_err();
        let cancelled = started.elapsed();
        assert_eq!(err, "Aborted");
        assert!(!out.join("big.bin").exists());
        assert!(
            cancelled * 2 < full,
            "cancel took {cancelled:?}, a full extraction {full:?}"
        );
    }

    /// Run by hand against a folder on a file system zipfast's own staged output refuses
    /// (HFS+, exFAT, NFS, SMB, ZFS, overlayfs, FUSE): `RUSTYGET_ZIP_FS_DIR=/Volumes/hfs
    /// TMPDIR=/Volumes/hfs/tmp cargo test --lib zip_extracts_on_any_file_system -- --ignored`
    /// (TMPDIR there too makes zipfast refuse every AES spool folder).
    #[test]
    #[ignore]
    fn test_zip_extracts_on_any_file_system() {
        let Some(base) = std::env::var_os("RUSTYGET_ZIP_FS_DIR") else {
            return;
        };
        let base = PathBuf::from(base).join(format!("rg_zip_fs_{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let payload = b"any file system";
        let zip_path = base.join("fs.zip");
        std::fs::write(
            &zip_path,
            build_single_entry_zip(
                b"a.txt",
                0,
                0,
                crc32(payload),
                payload.len() as u32,
                &[],
                payload,
            ),
        )
        .unwrap();
        for mode in [0, 1, 2] {
            let out = base.join(format!("out{mode}"));
            extract_zip_cancellable(&zip_path, &out, "", mode, &mut |_, _| {}, &mut || false)
                .unwrap();
            assert_eq!(
                std::fs::read(out.join("a.txt")).unwrap(),
                payload,
                "mode {mode}"
            );
        }

        // A WinZip AES member over the 64 KiB spool threshold. With TMPDIR also on such a
        // file system zipfast refuses every spool folder, so it is read in memory.
        let plain: Vec<u8> = (0..300_000u32).map(|i| (i * 13 % 251) as u8).collect();
        let password = "pw";
        let salt = [0x55u8; 16];
        let derived = zipfast::crypto::aes_derive(password.as_bytes(), &salt, 32).unwrap();
        let ct = zipfast::crypto::decrypt_winzip_ctr(&derived[..32], &plain).unwrap();
        let auth = {
            let key = openssl::pkey::PKey::hmac(&derived[32..64]).unwrap();
            let mut signer =
                openssl::sign::Signer::new(openssl::hash::MessageDigest::sha1(), &key).unwrap();
            signer.update(&ct).unwrap();
            signer.sign_to_vec().unwrap()
        };
        let mut aes_payload = salt.to_vec();
        aes_payload.extend_from_slice(&derived[64..66]);
        aes_payload.extend_from_slice(&ct);
        aes_payload.extend_from_slice(&auth[..10]);
        let mut extra = Vec::new();
        extra.extend_from_slice(&0x9901u16.to_le_bytes());
        extra.extend_from_slice(&7u16.to_le_bytes());
        extra.extend_from_slice(&2u16.to_le_bytes());
        extra.extend_from_slice(b"AE");
        extra.push(3);
        extra.extend_from_slice(&0u16.to_le_bytes());
        let aes_zip = base.join("aes.zip");
        std::fs::write(
            &aes_zip,
            build_single_entry_zip(
                b"big.bin",
                1,
                99,
                0,
                plain.len() as u32,
                &extra,
                &aes_payload,
            ),
        )
        .unwrap();
        let out = base.join("out_aes");
        extract_zip_cancellable(&aes_zip, &out, password, 1, &mut |_, _| {}, &mut || false)
            .unwrap();
        assert_eq!(std::fs::read(out.join("big.bin")).unwrap(), plain);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_zip_method_95_xz_extraction() {
        let plain = b"ZIP method 95 XZ stream payload test!";
        let plain_crc = crc32(plain);

        // Build a minimal valid XZ stream (CRC64 check, 1 block with 1 LZMA2 filter, uncompressed LZMA2 chunk)
        let mut xz = Vec::new();
        xz.extend_from_slice(b"\xfd7zXZ\x00");
        let stream_flags = [0x00u8, 0x04]; // CRC64
        xz.extend_from_slice(&stream_flags);
        xz.extend_from_slice(&crc32(&stream_flags).to_le_bytes());

        // Block header: indicator=0x02 => size=(2+1)*4=12 bytes (8 bytes body + 4 bytes CRC32)
        let mut blk_hdr = vec![0x02u8, 0x00, 0x21, 0x01, 0x00, 0x00, 0x00, 0x00];
        let blk_hdr_crc = crc32(&blk_hdr);
        blk_hdr.extend_from_slice(&blk_hdr_crc.to_le_bytes());
        xz.extend_from_slice(&blk_hdr);

        // LZMA2 data: control=0x01 (uncompressed, reset dict), 2-byte big-endian size-1, raw data, control=0x00 (end)
        let mut lzma2_data = Vec::new();
        lzma2_data.push(0x01);
        lzma2_data.extend_from_slice(&((plain.len() as u16) - 1).to_be_bytes());
        lzma2_data.extend_from_slice(plain);
        lzma2_data.push(0x00);
        let comp_bytes = lzma2_data.len();
        xz.extend_from_slice(&lzma2_data);
        let pad_len = (4 - (comp_bytes % 4)) % 4;
        xz.resize(xz.len() + pad_len, 0);

        // Block check (CRC64/XZ)
        // CRC-64/XZ, here only to build the fixture.
        let crc64 = !plain.iter().fold(u64::MAX, |mut crc, &b| {
            crc ^= u64::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xC96C_5795_D787_0F42
                } else {
                    crc >> 1
                };
            }
            crc
        });
        xz.extend_from_slice(&crc64.to_le_bytes());

        // Index: indicator=0x00, num_records=1, unpadded_size=12+comp_bytes+8, unpacked_size=plain.len()
        let unpadded = (12 + comp_bytes + 8) as u8;
        let mut idx = vec![0x00u8, 0x01, unpadded, plain.len() as u8];
        let idx_pad = (4 - (idx.len() % 4)) % 4;
        idx.resize(idx.len() + idx_pad, 0);
        let idx_crc = crc32(&idx);
        xz.extend_from_slice(&idx);
        xz.extend_from_slice(&idx_crc.to_le_bytes());

        // Stream footer: CRC32, backward_size = (idx.len() + 4) / 4 - 1, stream_flags, "YZ"
        let bw = (((idx.len() + 4) / 4) - 1) as u32;
        let mut footer_body = Vec::new();
        footer_body.extend_from_slice(&bw.to_le_bytes());
        footer_body.extend_from_slice(&stream_flags);
        let footer_crc = crc32(&footer_body);
        xz.extend_from_slice(&footer_crc.to_le_bytes());
        xz.extend_from_slice(&footer_body);
        xz.extend_from_slice(b"YZ");

        let zip_bytes = build_single_entry_zip(
            b"hello.xz.txt",
            0,
            95,
            plain_crc,
            plain.len() as u32,
            &[],
            &xz,
        );
        let dir = TempTestDir::new("aes_zc");
        let zip_path = dir.path().join("xz95.zip");
        let out_dir = dir.path().join("out_xz95");
        std::fs::write(&zip_path, &zip_bytes).unwrap();

        let summary =
            extract_zip_cancellable(&zip_path, &out_dir, "", 0, &mut |_, _| {}, &mut || false)
                .unwrap();
        assert_eq!(summary.files, 1);
        assert_eq!(std::fs::read(out_dir.join("hello.xz.txt")).unwrap(), plain);

        // Verify an XZ block header with an oversized filter-property length (e.g. ~4 GiB vint)
        // is rejected immediately before allocating.
        let mut bad_xz = Vec::new();
        bad_xz.extend_from_slice(b"\xfd7zXZ\x00");
        bad_xz.extend_from_slice(&stream_flags);
        bad_xz.extend_from_slice(&crc32(&stream_flags).to_le_bytes());
        // 12-byte block header with fid=0x21 and psz=0x0f_ffff_ffff (5-byte vint)
        let mut bad_blk_hdr = vec![0x02u8, 0x00, 0x21, 0xff, 0xff, 0xff, 0xff, 0x0f];
        let bad_blk_crc = crc32(&bad_blk_hdr);
        bad_blk_hdr.extend_from_slice(&bad_blk_crc.to_le_bytes());
        bad_xz.extend_from_slice(&bad_blk_hdr);
        let bad_zip = build_single_entry_zip(
            b"bad.xz.txt",
            0,
            95,
            plain_crc,
            plain.len() as u32,
            &[],
            &bad_xz,
        );
        let bad_zip_path = dir.path().join("xz95_bad.zip");
        std::fs::write(&bad_zip_path, &bad_zip).unwrap();
        let bad_out = dir.path().join("out_xz95_bad");
        let err =
            extract_zip_cancellable(&bad_zip_path, &bad_out, "", 0, &mut |_, _| {}, &mut || {
                false
            })
            .unwrap_err();
        assert!(err.contains("bad.xz.txt"), "unexpected: {err}");
        assert!(!bad_out.join("bad.xz.txt").exists());
    }

    #[test]
    fn test_rar4_terminal_end_marker_ignores_stale_next_volume() {
        let dir = TempTestDir::new("rar4_stale_vol");
        let payload = b"complete single volume payload";
        let payload_crc = crc32(payload);
        let name = b"hello.txt";

        let mut rar = Vec::new();
        rar.extend_from_slice(MARK4);

        // MAIN_HEAD (0x73), flags=0x0001 (MHD_VOLUME set, e.g. part1)
        let mut main_body = vec![0x73u8, 0x01, 0x00, 13, 0x00, 0, 0, 0, 0, 0, 0];
        let main_crc = (crc32(&main_body) as u16).to_le_bytes();
        rar.extend_from_slice(&main_crc);
        rar.extend_from_slice(&main_body);
        main_body.clear();

        // FILE_HEAD (0x74), flags=0x8000 (has add_size, NOT split after), method=0x30 (stored)
        let hdr_size = (32 + name.len()) as u16;
        let mut fh = Vec::new();
        fh.push(0x74);
        fh.extend_from_slice(&0x8000u16.to_le_bytes());
        fh.extend_from_slice(&hdr_size.to_le_bytes());
        fh.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        fh.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        fh.push(3); // Unix
        fh.extend_from_slice(&payload_crc.to_le_bytes());
        fh.extend_from_slice(&0u32.to_le_bytes());
        fh.push(29);
        fh.push(0x30); // Stored
        fh.extend_from_slice(&(name.len() as u16).to_le_bytes());
        fh.extend_from_slice(&0o644u32.to_le_bytes());
        fh.extend_from_slice(name);
        let fh_crc = (crc32(&fh) as u16).to_le_bytes();
        rar.extend_from_slice(&fh_crc);
        rar.extend_from_slice(&fh);
        rar.extend_from_slice(payload);

        // ENDARC_HEAD (0x7b), flags=0x0000 (terminal volume: EARC_NEXT_VOLUME is NOT set)
        let end_body = [0x7bu8, 0x00, 0x00, 7, 0x00];
        let end_crc = (crc32(&end_body) as u16).to_le_bytes();
        rar.extend_from_slice(&end_crc);
        rar.extend_from_slice(&end_body);

        let vol1 = dir.path().join("set.part1.rar");
        let stale_vol2 = dir.path().join("set.part2.rar");
        std::fs::write(&vol1, &rar).unwrap();
        // Write garbage to stale_vol2 so opening it would fail extraction
        std::fs::write(&stale_vol2, b"corrupt leftover volume from old download").unwrap();

        let out_dir = dir.path().join("out");
        let summary =
            extract_rar4_cancellable(&vol1, &out_dir, "", 1, &mut |_, _| {}, &mut || false)
                .unwrap();
        assert_eq!(summary.files, 1);
        assert_eq!(std::fs::read(out_dir.join("hello.txt")).unwrap(), payload);
    }

    #[test]
    fn test_rar4_backslash_separates_directories_on_every_host() {
        for windows in [false, true] {
            assert_eq!(
                sanitize_rar4_rel_path_bytes(b"dir\\file.bin", windows).unwrap(),
                PathBuf::from("dir").join("file.bin")
            );
        }
    }

    #[test]
    fn test_rar4_preserves_distinct_non_utf8_member_names() {
        let dir = TempTestDir::new("rar4_non_utf8");
        let append_rar4_stored_member = |rar: &mut Vec<u8>, name: &[u8], payload: &[u8]| {
            let hdr_size = (32 + name.len()) as u16;
            let mut fh = Vec::new();
            fh.push(0x74);
            fh.extend_from_slice(&0x8000u16.to_le_bytes()); // LHD_LARGE=0, LHD_UNICODE=0
            fh.extend_from_slice(&hdr_size.to_le_bytes());
            fh.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            fh.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            fh.push(3); // Unix
            fh.extend_from_slice(&crc32(payload).to_le_bytes());
            fh.extend_from_slice(&0u32.to_le_bytes());
            fh.push(29);
            fh.push(0x30); // Stored
            fh.extend_from_slice(&(name.len() as u16).to_le_bytes());
            fh.extend_from_slice(&0o644u32.to_le_bytes());
            fh.extend_from_slice(name);
            let fh_crc = (crc32(&fh) as u16).to_le_bytes();
            rar.extend_from_slice(&fh_crc);
            rar.extend_from_slice(&fh);
            rar.extend_from_slice(payload);
        };

        let mut rar = Vec::new();
        rar.extend_from_slice(MARK4);
        let main_body = [0x73u8, 0x00, 0x00, 13, 0x00, 0, 0, 0, 0, 0, 0];
        let main_crc = (crc32(&main_body) as u16).to_le_bytes();
        rar.extend_from_slice(&main_crc);
        rar.extend_from_slice(&main_body);

        append_rar4_stored_member(&mut rar, b"caf\xe9.txt", b"first");
        append_rar4_stored_member(&mut rar, b"caf\xe8.txt", b"second");

        let end_body = [0x7bu8, 0x00, 0x00, 7, 0x00];
        let end_crc = (crc32(&end_body) as u16).to_le_bytes();
        rar.extend_from_slice(&end_crc);
        rar.extend_from_slice(&end_body);

        let rar_path = dir.path().join("legacy.rar");
        let out_dir = dir.path().join("out");
        std::fs::write(&rar_path, &rar).unwrap();

        let summary = extract_rar4_cancellable(
            &rar_path,
            &out_dir,
            "",
            1, // overwrite=true
            &mut |_, _| {},
            &mut || false,
        )
        .unwrap();
        assert_eq!(summary.files, 2);
        let rel1 = sanitize_rar4_rel_path_bytes(b"caf\xe9.txt", false).unwrap();
        let rel2 = sanitize_rar4_rel_path_bytes(b"caf\xe8.txt", false).unwrap();
        assert_ne!(rel1, rel2);
        assert_eq!(std::fs::read(out_dir.join(rel1)).unwrap(), b"first");
        assert_eq!(std::fs::read(out_dir.join(rel2)).unwrap(), b"second");
    }

    #[test]
    fn test_tar_preserves_distinct_non_utf8_member_names() {
        let make_raw_tar_header = |name: &[u8], size: u64| -> [u8; 512] {
            let mut h = [0u8; 512];
            h[..name.len()].copy_from_slice(name);
            h[100..107].copy_from_slice(b"0000644");
            h[108..115].copy_from_slice(b"0000000");
            h[116..123].copy_from_slice(b"0000000");
            let sz_oct = format!("{size:011o}");
            h[124..135].copy_from_slice(sz_oct.as_bytes());
            h[136..147].copy_from_slice(b"00000000000");
            h[148..156].fill(b' ');
            h[156] = b'0';
            h[257..263].copy_from_slice(b"ustar\0");
            h[263..265].copy_from_slice(b"00");
            let csum: u32 = h.iter().map(|&b| u32::from(b)).sum();
            let csum_str = format!("{csum:06o}\0 ");
            h[148..156].copy_from_slice(csum_str.as_bytes());
            h
        };

        let rel1 = sanitize_tar_rel_path_bytes(b"caf\xe9.txt").unwrap();
        let rel2 = sanitize_tar_rel_path_bytes(b"caf\xe8.txt").unwrap();
        let rel3 = sanitize_tar_rel_path_bytes("café.txt".as_bytes()).unwrap();
        assert_ne!(rel1, rel2);
        assert_ne!(rel1, rel3);
        #[cfg(all(unix, not(target_vendor = "apple")))]
        {
            use std::os::unix::ffi::OsStrExt;
            assert_eq!(rel1.as_os_str().as_bytes(), b"caf\xe9.txt");
            assert_eq!(rel2.as_os_str().as_bytes(), b"caf\xe8.txt");
        }

        let mut tar = Vec::new();
        tar.extend_from_slice(&make_raw_tar_header(b"caf\xe9.txt", 5));
        let mut b1 = [0u8; 512];
        b1[..5].copy_from_slice(b"first");
        tar.extend_from_slice(&b1);

        tar.extend_from_slice(&make_raw_tar_header(b"caf\xe8.txt", 6));
        let mut b2 = [0u8; 512];
        b2[..6].copy_from_slice(b"second");
        tar.extend_from_slice(&b2);
        tar.extend_from_slice(&[0u8; 1024]);

        let dir = TempTestDir::new("tar_non_utf8");
        let tar_path = dir.path().join("non_utf8.tar");
        let out_dir = dir.path().join("out");
        std::fs::write(&tar_path, &tar).unwrap();

        let summary = extract_tar_or_stream_cancellable(
            &tar_path,
            &out_dir,
            1, // overwrite=true
            &mut |_, _| {},
            &mut || false,
        )
        .unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(std::fs::read(out_dir.join(&rel1)).unwrap(), b"first");
        assert_eq!(std::fs::read(out_dir.join(&rel2)).unwrap(), b"second");
    }

    fn make_tar_header(name: &str, size: u64, typeflag: u8) -> [u8; 512] {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[108..115].copy_from_slice(b"0000000");
        h[116..123].copy_from_slice(b"0000000");
        let sz_oct = format!("{size:011o}");
        h[124..135].copy_from_slice(sz_oct.as_bytes());
        h[136..147].copy_from_slice(b"00000000000");
        h[148..156].fill(b' ');
        h[156] = typeflag;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        let csum: u32 = h.iter().map(|&b| u32::from(b)).sum();
        let csum_str = format!("{csum:06o}\0 ");
        h[148..156].copy_from_slice(csum_str.as_bytes());
        h
    }

    #[test]
    fn test_tar_rejects_gnu_and_pax_sparse_members() {
        let dir = TempTestDir::new("aes_zc");

        // 1. GNU tar sparse member (typeflag == b'S')
        let mut gnu_tar = Vec::new();
        gnu_tar.extend_from_slice(&make_tar_header("sparse.bin", 4, b'S'));
        let mut data_blk = [0u8; 512];
        data_blk[..4].copy_from_slice(b"data");
        gnu_tar.extend_from_slice(&data_blk);
        gnu_tar.extend_from_slice(&[0u8; 1024]);

        let gnu_path = dir.path().join("gnu_sparse.tar");
        let gnu_out = dir.path().join("gnu_out");
        std::fs::write(&gnu_path, &gnu_tar).unwrap();
        let err =
            extract_tar_or_stream_cancellable(&gnu_path, &gnu_out, 0, &mut |_, _| {}, &mut || {
                false
            })
            .unwrap_err();
        assert!(
            err.contains("sparse files are not supported"),
            "unexpected: {err}"
        );
        assert!(!gnu_out.join("sparse.bin").exists());

        // 2. PAX sparse member (typeflag == b'x' with GNU.sparse.major=1 followed by regular b'0')
        let pax_rec = b"22 GNU.sparse.major=1\n";
        let mut pax_tar = Vec::new();
        pax_tar.extend_from_slice(&make_tar_header(
            "PaxHeader/sparse2.bin",
            pax_rec.len() as u64,
            b'x',
        ));
        let mut pax_blk = [0u8; 512];
        pax_blk[..pax_rec.len()].copy_from_slice(pax_rec);
        pax_tar.extend_from_slice(&pax_blk);
        pax_tar.extend_from_slice(&make_tar_header("sparse2.bin", 4, b'0'));
        pax_tar.extend_from_slice(&data_blk);
        pax_tar.extend_from_slice(&[0u8; 1024]);

        let pax_path = dir.path().join("pax_sparse.tar");
        let pax_out = dir.path().join("pax_out");
        std::fs::write(&pax_path, &pax_tar).unwrap();
        let err2 =
            extract_tar_or_stream_cancellable(&pax_path, &pax_out, 0, &mut |_, _| {}, &mut || {
                false
            })
            .unwrap_err();
        assert!(
            err2.contains("sparse files are not supported"),
            "unexpected: {err2}"
        );
        assert!(!pax_out.join("sparse2.bin").exists());
    }

    #[test]
    fn test_tar_gz_validates_compressed_trailer_after_terminator() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let mut tar_bytes = Vec::new();
        tar_bytes.extend_from_slice(&make_tar_header("ok.txt", 5, b'0'));
        let mut blk = [0u8; 512];
        blk[..5].copy_from_slice(b"hello");
        tar_bytes.extend_from_slice(&blk);
        // Add 20 zero blocks (standard 10 KiB record padding) after the member
        tar_bytes.extend_from_slice(&[0u8; 512 * 20]);

        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&tar_bytes).unwrap();
        let gz_bytes = enc.finish().unwrap();

        let dir = TempTestDir::new("aes_zc");
        let good_path = dir.path().join("good.tar.gz");
        let good_out = dir.path().join("good_out");
        std::fs::write(&good_path, &gz_bytes).unwrap();
        let summary = extract_tar_or_stream_cancellable(
            &good_path,
            &good_out,
            0,
            &mut |_, _| {},
            &mut || false,
        )
        .unwrap();
        assert_eq!(summary.files, 1);
        assert_eq!(std::fs::read(good_out.join("ok.txt")).unwrap(), b"hello");

        // Corrupt the final 8-byte gzip trailer (CRC32 + ISIZE) after the TAR terminator
        let mut bad_gz = gz_bytes.clone();
        let len = bad_gz.len();
        bad_gz[len - 5] ^= 0xff;
        let bad_path = dir.path().join("bad.tar.gz");
        let bad_out = dir.path().join("bad_out");
        std::fs::write(&bad_path, &bad_gz).unwrap();
        let err =
            extract_tar_or_stream_cancellable(&bad_path, &bad_out, 0, &mut |_, _| {}, &mut || {
                false
            })
            .unwrap_err();
        assert!(
            err.contains("Corrupt compressed tar trailer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_lzma2_dict_cap_preserves_large_dictionary_property() {
        // bits = 36 => (2 | 0) << (18 + 11) = 1 GiB
        assert_eq!(sz_lzma2_dict_cap(&[36]).unwrap(), 1usize << 30);
        // bits = 38 => (2 | 0) << (19 + 11) = 2 GiB
        assert_eq!(sz_lzma2_dict_cap(&[38]).unwrap(), 1usize << 31);
        // bits = 40 => u32::MAX (4 GiB - 1)
        assert_eq!(sz_lzma2_dict_cap(&[40]).unwrap(), u32::MAX as usize);
    }

    #[test]
    fn test_7z_streaming_compressed_solid_folder() {
        // Build a 2-file solid 7z archive using LZMA2 (coder id [0x21]) with an uncompressed LZMA2 chunk.
        let file1 = b"First file in solid LZMA2 folder!\n";
        let file2 = b"Second file in solid LZMA2 folder!\n";
        let mut solid_plain = Vec::new();
        solid_plain.extend_from_slice(file1);
        solid_plain.extend_from_slice(file2);

        // LZMA2 uncompressed chunk with dictionary reset (control = 0x01, 2-byte big-endian size - 1, data, 0x00 EOF)
        let mut packed = Vec::new();
        packed.push(0x01);
        let len_m1 = (solid_plain.len() - 1) as u16;
        packed.extend_from_slice(&len_m1.to_be_bytes());
        packed.extend_from_slice(&solid_plain);
        packed.push(0x00);

        let mut hdr = Vec::new();
        hdr.push(0x01); // Header
        hdr.push(0x04); // MainStreamsInfo
                        // PackInfo (0x06)
        hdr.push(0x06);
        hdr.push(0x00); // pack_pos = 0
        hdr.push(0x01); // num_pack_streams = 1
        hdr.push(0x09); // Size
        hdr.push(packed.len() as u8);
        hdr.push(0x00); // End PackInfo

        // UnpackInfo (0x07)
        hdr.push(0x07);
        hdr.push(0x0b); // Folder
        hdr.push(0x01); // num_folders = 1
        hdr.push(0x00); // external = 0
        hdr.push(0x01); // num_coders = 1
        hdr.push(0x21); // coder flags: id_size=1, has_props=true (0x20 | 0x01)
        hdr.push(0x21); // LZMA2 coder ID
        hdr.push(0x01); // props size = 1
        hdr.push(0x18); // dict prop
        hdr.push(0x0c); // CodersUnpackSize
        hdr.push(solid_plain.len() as u8);
        hdr.push(0x00); // End UnpackInfo

        // SubStreamsInfo (0x08)
        hdr.push(0x08);
        hdr.push(0x0d); // NumUnpackStream
        hdr.push(0x02); // 2 files in folder 0
        hdr.push(0x09); // Size (first N-1 sizes)
        hdr.push(file1.len() as u8);
        hdr.push(0x0a); // CRC
        hdr.push(0x01); // all defined
        hdr.extend_from_slice(&crc32(file1).to_le_bytes());
        hdr.extend_from_slice(&crc32(file2).to_le_bytes());
        hdr.push(0x00); // End SubStreamsInfo
        hdr.push(0x00); // End MainStreamsInfo

        // FilesInfo (0x05)
        hdr.push(0x05);
        hdr.push(0x02); // 2 files
        hdr.push(0x11); // Name
        let mut names_utf16 = Vec::new();
        names_utf16.push(0x00); // external = 0
        for name in ["alpha.txt", "beta.txt"] {
            for u in name.encode_utf16() {
                names_utf16.extend_from_slice(&u.to_le_bytes());
            }
            names_utf16.extend_from_slice(&0u16.to_le_bytes());
        }
        hdr.push(names_utf16.len() as u8);
        hdr.extend_from_slice(&names_utf16);
        hdr.push(0x00); // End FilesInfo
        hdr.push(0x00); // End Header

        let next_offset = packed.len() as u64;
        let next_size = hdr.len() as u64;
        let next_crc = crc32(&hdr);
        let mut start_hdr = [0u8; 20];
        start_hdr[0..8].copy_from_slice(&next_offset.to_le_bytes());
        start_hdr[8..16].copy_from_slice(&next_size.to_le_bytes());
        start_hdr[16..20].copy_from_slice(&next_crc.to_le_bytes());
        let start_crc = crc32(&start_hdr);

        let mut archive = Vec::new();
        archive.extend_from_slice(SEVEN_Z);
        archive.extend_from_slice(&[0x00, 0x04]);
        archive.extend_from_slice(&start_crc.to_le_bytes());
        archive.extend_from_slice(&start_hdr);
        archive.extend_from_slice(&packed);
        archive.extend_from_slice(&hdr);

        let dir = TempTestDir::new("7z_stream_solid");
        let sz_path = dir.path().join("solid.7z");
        let out_dir = dir.path().join("out");
        std::fs::write(&sz_path, &archive).unwrap();

        let summary =
            extract_7z_cancellable(&sz_path, &out_dir, "", 1, &mut |_, _| {}, &mut || false)
                .unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(std::fs::read(out_dir.join("alpha.txt")).unwrap(), file1);
        assert_eq!(std::fs::read(out_dir.join("beta.txt")).unwrap(), file2);
    }

    #[test]
    fn test_7z_rejects_duplicate_coder_bindings_without_panicking() {
        // 1. Verify sz_folder rejects duplicate output bindings in bind_pairs
        // 2 coders: coder 0 (1 in, 1 out), coder 1 (4 in, 1 out -> total 5 in, 2 out -> 1 bind pair)
        // If coder 1 binds two of its inputs to out_idx 0 (when out_total=3), sz_folder rejects it.
        let mut raw = Vec::new();
        raw.push(0x03); // 3 coders
        raw.push(0x01);
        raw.push(0x00); // coder 0: id=[0x00], 1 in, 1 out
        raw.push(0x01);
        raw.push(0x00); // coder 1: id=[0x00], 1 in, 1 out
        raw.push(0x14);
        raw.extend_from_slice(&[0x03, 0x03, 0x01, 0x1b]); // coder 2: BCJ2
        raw.push(0x04); // 4 in
        raw.push(0x01); // 1 out
                        // pairs = out_total - 1 = 2; bind both in=2 and in=3 to out=0 (duplicate out_idx!)
        raw.extend_from_slice(&[2, 0, 3, 0]);
        raw.extend_from_slice(&[0, 1, 4, 5]);
        let mut r = SzReader { b: &raw, p: 0 };
        assert!(sz_folder(&mut r).is_none());

        // 2. Also verify sz_decode_folder_streaming returns Err rather than panicking
        // if handed a manually constructed SzFolder with duplicate output bindings.
        let bad_folder = SzFolder {
            coders: vec![
                SzCoder {
                    id: vec![0x00],
                    props: Vec::new(),
                    num_in: 1,
                    num_out: 1,
                },
                SzCoder {
                    id: vec![0x03, 0x03, 0x01, 0x1b],
                    props: Vec::new(),
                    num_in: 4,
                    num_out: 1,
                },
            ],
            bind_pairs: vec![(1, 0), (2, 0)],
            packed_streams: vec![0, 3, 4],
            unpack: vec![4, 4],
            crc: None,
            out_streams: 2,
        };
        let slices: [&[u8]; 3] = [b"abcd", b"efgh", b"ijkl"];
        let err =
            sz_decode_folder_streaming(&bad_folder, &slices, "", &mut |_| Ok(())).unwrap_err();
        assert!(
            err.contains("Corrupt or duplicate coder input binding")
                || err.contains("Unresolvable"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_rar4_restores_member_mtime() {
        let dir = TempTestDir::new("rar4_mtime");
        let rar_path = dir.path().join("mtime.rar");
        let out_dir = dir.path().join("out");

        // DOS timestamp 0x5D32_86A0 = 2026-09-18 16:53:00 UTC = 1_789_750_380
        // Extended time [0x00, 0xC0] adds +1 second -> 1_789_750_381
        let payload = b"timestamped rar4 payload";
        let name = b"dated.txt";
        let mut archive = MARK4.to_vec();

        // Main archive header (0x73)
        let mut main_hdr = vec![0, 0, 0x73, 0x00, 0x00, 13, 0, 0, 0, 0, 0, 0, 0];
        let main_crc = (crc32(&main_hdr[2..]) & 0xffff) as u16;
        main_hdr[0..2].copy_from_slice(&main_crc.to_le_bytes());
        archive.extend_from_slice(&main_hdr);

        // File header (0x74) with LHD_WINDOW64 (0x8000) | LHD_EXTTIME (0x1000)
        let flags: u16 = 0x8000 | 0x1000;
        let ext_time = [0x00u8, 0xC0]; // +1s on mtime, 0 fraction bytes
        let hdr_size = (32 + name.len() + ext_time.len()) as u16;
        let mut file_hdr = Vec::with_capacity(usize::from(hdr_size));
        file_hdr.extend_from_slice(&[0, 0]); // CRC16 placeholder
        file_hdr.push(0x74);
        file_hdr.extend_from_slice(&flags.to_le_bytes());
        file_hdr.extend_from_slice(&hdr_size.to_le_bytes());
        file_hdr.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        file_hdr.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        file_hdr.push(3); // Unix host OS
        file_hdr.extend_from_slice(&crc32(payload).to_le_bytes());
        file_hdr.extend_from_slice(&0x5D32_86A0u32.to_le_bytes()); // DOS time
        file_hdr.push(29); // RAR 2.9
        file_hdr.push(0x30); // Store
        file_hdr.extend_from_slice(&(name.len() as u16).to_le_bytes());
        file_hdr.extend_from_slice(&0o100644u32.to_le_bytes());
        file_hdr.extend_from_slice(name);
        file_hdr.extend_from_slice(&ext_time);
        let file_hdr_crc = (crc32(&file_hdr[2..]) & 0xffff) as u16;
        file_hdr[0..2].copy_from_slice(&file_hdr_crc.to_le_bytes());
        archive.extend_from_slice(&file_hdr);
        archive.extend_from_slice(payload);

        // End of archive header (0x7b)
        let mut end_hdr = vec![0, 0, 0x7b, 0x00, 0x00, 7, 0];
        let end_crc = (crc32(&end_hdr[2..]) & 0xffff) as u16;
        end_hdr[0..2].copy_from_slice(&end_crc.to_le_bytes());
        archive.extend_from_slice(&end_hdr);

        std::fs::write(&rar_path, &archive).unwrap();
        let summary =
            extract_rar4_cancellable(&rar_path, &out_dir, "", 1, &mut |_, _| {}, &mut || false)
                .unwrap();
        assert_eq!(summary.files, 1);
        let extracted = out_dir.join("dated.txt");
        assert_eq!(std::fs::read(&extracted).unwrap(), payload);
        let mtime_secs = std::fs::metadata(&extracted)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(mtime_secs, 1_789_750_381);
    }
}
