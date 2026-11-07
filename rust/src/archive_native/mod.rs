//! Native pure-Rust RAR 1.5/2.0/2.9 (RAR3/4) and 7z (`.7z`, `.7z.001`) archive extractors.

pub mod bcj;
pub mod heap;
pub mod lzma;
pub mod ppmd_h;
pub mod rar15;
pub mod rar20;
pub mod rar3;
pub mod rarbits;

use std::io::Write;
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
    Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

#[inline]
fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

#[inline]
fn le64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at.checked_add(8)?)?.try_into().ok()?))
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

fn unicode4(name: &[u8], enc: &[u8]) -> String {
    let mut bytes = enc.iter().copied();
    let Some(high) = bytes.next() else {
        return String::from_utf8_lossy(name).into_owned();
    };
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
                let (Some(a), Some(b)) = (bytes.next(), bytes.next()) else { break };
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
                    let Some(c) = name.get(out.len()).copied() else { break };
                    out.push(match correction {
                        Some(k) => u16::from(c.wrapping_add(k)) | high,
                        None => u16::from(c),
                    });
                }
            }
        }
    }
    String::from_utf16_lossy(&out)
}

fn name4(raw: &[u8], flags: u16) -> String {
    if flags & 0x0200 != 0 {
        if let Some(nul) = raw.iter().position(|b| *b == 0) {
            if let (Some(name), Some(enc)) = (raw.get(..nul), raw.get(nul + 1..)) {
                return unicode4(name, enc);
            }
        }
    }
    String::from_utf8_lossy(raw.split(|b| *b == 0).next().unwrap_or(raw)).into_owned()
}

#[derive(Clone, Debug)]
struct Rar4FileChunk {
    name: String,
    unpacked_size: u64,
    crc32: u32,
    rar_version: u8,
    method: u8,
    is_dir: bool,
    is_symlink: bool,
    unix_mode: Option<u32>,
    solid: bool,
    encrypted: bool,
    salt: Option<[u8; 8]>,
    split_before: bool,
    continues: bool,
    windows_names: bool,
    data: Vec<u8>,
}

struct ParsedRar4Volume {
    chunks: Vec<Rar4FileChunk>,
    has_next_volume: bool,
}

fn parse_rar4_volume_chunks(
    vol_bytes: &[u8],
    password: &str,
    cache: &mut Rar4KeyCache,
) -> Result<ParsedRar4Volume, String> {
    if !vol_bytes.starts_with(MARK4) {
        return Err("Not a RAR 1.5-4.x volume".to_string());
    }
    let mut chunks = Vec::new();
    let mut pos = MARK4.len();
    let mut encrypted_headers = false;
    let mut reached_end = false;
    let mut end_has_next_volume = false;

    for _ in 0..MAX_BLOCKS {
        if pos >= vol_bytes.len() {
            reached_end = true;
            break;
        }
        let (hdr_bytes, header, block_salt) = if encrypted_headers {
            if password.is_empty() {
                return Err("Unrar: The specified password is incorrect.".to_string());
            }
            let Some((hb, hdr, bs)) = decrypt_rar4_header(vol_bytes, pos, password, cache) else {
                return Err("Unrar: The specified password is incorrect.".to_string());
            };
            (hb, hdr, Some(bs))
        } else {
            let Some(rest) = vol_bytes.get(pos..) else {
                return Err("Unrar: Unexpected end of archive header".to_string());
            };
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
            end_has_next_volume = (flags & 0x0001) != 0;
            reached_end = true;
            break;
        }
        if ty == 0x73 && !encrypted_headers && (flags & 0x0080 != 0) {
            encrypted_headers = true;
            pos = pos.saturating_add(hdr_bytes);
            continue;
        }

        let mut add = 0u64;
        if flags & 0x8000 != 0 {
            let Some(low) = le32(&header, 7) else {
                return Err("Unrar: Corrupt block header".to_string());
            };
            add = u64::from(low);
        }

        let data_at = pos.saturating_add(hdr_bytes);
        if ty == 0x74 {
            if header.len() < 32 {
                return Err("Unrar: Corrupt file header".to_string());
            }
            let unp_low = u64::from(le32(&header, 11).unwrap_or(0));
            let host_os = header.get(15).copied().unwrap_or(0);
            let file_crc = le32(&header, 16).unwrap_or(0);
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
            let name = name4(raw_name, flags);
            let mut salt = if flags & 0x0400 != 0 {
                header
                    .get(name_end..name_end + 8)
                    .and_then(|s| s.try_into().ok())
            } else {
                None
            };
            if salt.is_none() {
                salt = block_salt;
            }
            let packed_usize = usize::try_from(add).map_err(|_| "Unrar: Packed size overflow")?;
            let data_end = data_at
                .checked_add(packed_usize)
                .ok_or_else(|| "Unrar: Packed data offset overflow".to_string())?;
            let Some(chunk_slice) = vol_bytes.get(data_at..data_end) else {
                return Err("Unrar: Unexpected end of archive data".to_string());
            };
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
                unpacked_size,
                crc32: file_crc,
                rar_version,
                method,
                is_dir,
                is_symlink,
                unix_mode,
                solid: (flags & 0x0010) != 0,
                encrypted: (flags & 0x0004 != 0) || encrypted_headers,
                salt,
                split_before: (flags & 0x0001) != 0,
                continues: (flags & 0x0002) != 0,
                windows_names: host_os != 3,
                data: chunk_slice.to_vec(),
            });
        }

        let next = (data_at as u64).saturating_add(add);
        let Some(next_pos) = usize::try_from(next).ok() else {
            return Err("Unrar: Packed data offset overflow".to_string());
        };
        if next_pos <= pos || next_pos > vol_bytes.len() {
            return Err("Unrar: Unexpected end of archive data".to_string());
        }
        pos = next_pos;
    }

    if !reached_end {
        return Err("Unrar: Archive exceeds maximum block count".to_string());
    }

    let has_next_volume = end_has_next_volume || chunks.last().is_some_and(|c| c.continues);
    Ok(ParsedRar4Volume {
        chunks,
        has_next_volume,
    })
}

#[derive(Default)]
struct RarDecoderSuite {
    r15: rar15::Rar15Decoder,
    r20: rar20::Rar20Decoder,
    r29: rar3::Rar29Decoder,
}

impl RarDecoderSuite {
    fn decode(
        &mut self,
        packed: &[u8],
        method: u8,
        rar_version: u8,
        unpacked_size: u64,
        solid: bool,
    ) -> Result<Vec<u8>, String> {
        let target = usize::try_from(unpacked_size).map_err(|_| "Unpacked size overflow")?;
        if method == 0x30 {
            if packed.len() < target {
                return Err("Unrar: Stored stream shorter than unpacked size".to_string());
            }
            return Ok(packed[..target].to_vec());
        }
        match rar_version {
            15 => self.r15.decode(packed, target, solid),
            20 | 26 => self.r20.decode(packed, target, solid),
            _ => self.r29.decode(packed, target, solid),
        }
    }
}

fn resolve_dest_path(
    output_dir: &Path,
    sanitized_rel: &Path,
    overwrite_mode: i32,
) -> Option<PathBuf> {
    let candidate = output_dir.join(sanitized_rel);
    if !candidate.exists() {
        return Some(candidate);
    }
    match overwrite_mode {
        0 => None, // Skip
        2 => {
            // AutoRename
            let stem = candidate
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("file");
            let ext = candidate.extension().and_then(|s| s.to_str());
            let parent = candidate.parent().unwrap_or(output_dir);
            for n in 1..10_000u32 {
                let new_name = match ext {
                    Some(e) => format!("{stem}_{n}.{e}"),
                    None => format!("{stem}_{n}"),
                };
                let alt = parent.join(new_name);
                if !alt.exists() {
                    return Some(alt);
                }
            }
            Some(candidate)
        }
        _ => Some(candidate), // Overwrite
    }
}

#[cfg(unix)]
fn apply_unix_executable_mode(path: &Path, mode: u32) {
    if (mode & 0o111) != 0 {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode((mode & 0o777) | 0o600),
        );
    }
}

#[cfg(not(unix))]
fn apply_unix_executable_mode(_path: &Path, _mode: u32) {}

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
) -> Result<(), String>
where
    F: FnMut(i32, &str),
{
    while let Some(first) = pending.front() {
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
        let rel = zipfast::path::sanitize(&first.name, first.windows_names).map_err(|e| {
            let msg = format!("Unrar: {e}");
            log_cb(1, &msg);
            msg
        })?;
        if first.is_dir {
            let dir_path = output_dir.join(&rel);
            std::fs::create_dir_all(&dir_path).map_err(|e| e.to_string())?;
            summary.dirs += 1;
            continue;
        }
        if first.split_before {
            let msg =
                "Unrar: WARNING: You need to start extraction from a previous volume".to_string();
            log_cb(1, &msg);
            return Err(msg);
        }

        let mut combined_raw = first.data.clone();
        let mut per_vol_decrypted = Vec::new();
        let mut can_decrypt_per_vol = true;
        if first.encrypted {
            if password.is_empty() {
                let msg = "Unrar: The specified password is incorrect.".to_string();
                log_cb(1, &msg);
                return Err(msg);
            }
            if let Some(salt) = first.salt {
                let (key, iv) = cache.derive(password, &salt);
                if let Some(dec) = zipfast::crypto::decrypt_cbc(&key, &iv, &first.data) {
                    per_vol_decrypted.extend_from_slice(&dec);
                } else {
                    can_decrypt_per_vol = false;
                }
            } else {
                can_decrypt_per_vol = false;
            }
        }

        let mut last_chunk = first.clone();
        while last_chunk.continues {
            let Some(next_chunk) = pending.pop_front() else {
                let msg = format!(
                    "Unrar: {} : packed data CRC failed in volume",
                    first.name
                );
                log_cb(1, &msg);
                return Err(msg);
            };
            if !next_chunk.split_before {
                let msg = format!(
                    "Unrar: {} : packed data CRC failed in volume",
                    first.name
                );
                log_cb(1, &msg);
                return Err(msg);
            }
            combined_raw.extend_from_slice(&next_chunk.data);
            if first.encrypted && can_decrypt_per_vol {
                if let Some(salt) = next_chunk.salt.or(first.salt) {
                    let (key, iv) = cache.derive(password, &salt);
                    if let Some(dec) = zipfast::crypto::decrypt_cbc(&key, &iv, &next_chunk.data) {
                        per_vol_decrypted.extend_from_slice(&dec);
                    } else {
                        can_decrypt_per_vol = false;
                    }
                } else {
                    can_decrypt_per_vol = false;
                }
            }
            last_chunk = next_chunk;
        }

        log_cb(4, &format!("Unrar: Extracting  {}", first.name));

        let decoded = if first.encrypted {
            let mut decoded_ok = None;
            if let Some(salt) = first.salt {
                let (key, iv) = cache.derive(password, &salt);
                if let Some(dec_all) = zipfast::crypto::decrypt_cbc(&key, &iv, &combined_raw) {
                    let mut trial = RarDecoderSuite::default();
                    let dec_res = if first.solid {
                        decoders.decode(
                            &dec_all,
                            first.method,
                            first.rar_version,
                            last_chunk.unpacked_size,
                            true,
                        )
                    } else {
                        trial.decode(
                            &dec_all,
                            first.method,
                            first.rar_version,
                            last_chunk.unpacked_size,
                            false,
                        )
                    };
                    if let Ok(out) = dec_res {
                        if crc32(&out) == last_chunk.crc32 {
                            if !first.solid {
                                *decoders = trial;
                            }
                            decoded_ok = Some(out);
                        }
                    }
                }
            }
            if decoded_ok.is_none() && can_decrypt_per_vol {
                if let Ok(out) = decoders.decode(
                    &per_vol_decrypted,
                    first.method,
                    first.rar_version,
                    last_chunk.unpacked_size,
                    first.solid,
                ) {
                    if crc32(&out) == last_chunk.crc32 {
                        decoded_ok = Some(out);
                    }
                }
            }
            let Some(out) = decoded_ok else {
                let msg = format!("Unrar: Checksum error in the encrypted file {}", first.name);
                log_cb(1, &msg);
                let pw_msg = "Unrar: The specified password is incorrect.".to_string();
                log_cb(1, &pw_msg);
                return Err(pw_msg);
            };
            out
        } else {
            let out = decoders
                .decode(
                    &combined_raw,
                    first.method,
                    first.rar_version,
                    last_chunk.unpacked_size,
                    first.solid,
                )
                .map_err(|e| {
                    let crc_msg = format!("Unrar: {} - CRC failed", first.name);
                    log_cb(1, &crc_msg);
                    log_cb(1, &e);
                    crc_msg
                })?;
            if crc32(&out) != last_chunk.crc32 {
                let crc_msg = format!("Unrar: {} - CRC failed", first.name);
                log_cb(1, &crc_msg);
                return Err(crc_msg);
            }
            out
        };

        if let Some(dest_path) = resolve_dest_path(output_dir, &rel, overwrite_mode) {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut f = std::fs::File::create(&dest_path).map_err(|e| e.to_string())?;
            f.write_all(&decoded).map_err(|e| e.to_string())?;
            drop(f);
            if let Some(mode) = first.unix_mode {
                apply_unix_executable_mode(&dest_path, mode);
            }
            summary.files += 1;
            summary.bytes += decoded.len() as u64;
        } else {
            summary.skipped += 1;
        }
    }
    Ok(())
}

/// Extracts a RAR 1.5–4.x single or multi-volume archive into `output_dir`.
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
        let vol_name = cur_vol
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("archive.rar");
        log_cb(4, &format!("Unrar: Extracting from {vol_name}"));
        let vol_bytes = std::fs::read(&cur_vol).map_err(|e| {
            let msg = format!("Unrar: Could not read {}: {e}", cur_vol.display());
            log_cb(1, &msg);
            msg
        })?;
        let parsed = parse_rar4_volume_chunks(&vol_bytes, password, &mut cache).map_err(|e| {
            log_cb(1, &e);
            e
        })?;
        drop(vol_bytes);
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
        )?;

        let Some(next_vol) = zipfast::volume::next_volume(&cur_vol) else {
            if parsed.has_next_volume {
                let msg = "Unrar: Cannot find next volume".to_string();
                log_cb(1, &msg);
                return Err(msg);
            }
            reached_final_volume = true;
            break;
        };
        if !next_vol.exists() {
            if parsed.has_next_volume {
                let next_name = next_vol
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("next volume");
                log_cb(
                    4,
                    &format!("Unrar: Insert disk with {next_name} [C]ontinue, [Q]uit"),
                );
                if !next_vol.exists() {
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
        f.bind_pairs.push((in_idx, out_idx));
    }
    if packed > 1 {
        for _ in 0..packed {
            f.packed_streams.push(r.number()?);
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

fn sz_decrypt_aes256(props: &[u8], data: &[u8], password: &str) -> Result<Vec<u8>, String> {
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
        let mut pw_utf16 = Vec::with_capacity(password.len() * 2);
        for u in password.encode_utf16() {
            pw_utf16.extend_from_slice(&u.to_le_bytes());
        }
        let rounds = 1usize << num_cycles_power;
        let round_len = salt.len() + pw_utf16.len() + 8;
        let mut buf = Vec::with_capacity(rounds.saturating_mul(round_len));
        for round in 0..(rounds as u64) {
            buf.extend_from_slice(salt);
            buf.extend_from_slice(&pw_utf16);
            buf.extend_from_slice(&round.to_le_bytes());
        }
        zipfast::crypto::sha256(&buf)
    };

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
    let mut out = Vec::with_capacity(target.min(16 * 1024 * 1024));
    let mut prev_byte = 0u8;

    while out.len() < target {
        let Some(&b) = main_buf.get(main_pos) else {
            return Err("7-Zip: BCJ2 main stream ended early".to_string());
        };
        main_pos += 1;
        out.push(b);
        if out.len() >= target {
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
        let ip = (out.len() as u32).wrapping_add(4);
        let rel = val.wrapping_sub(ip);
        let rel_bytes = rel.to_le_bytes();
        let take_n = (target - out.len()).min(4);
        out.extend_from_slice(&rel_bytes[..take_n]);
        prev_byte = rel_bytes[3];
    }

    if out.len() != target {
        return Err("7-Zip: BCJ2 decompression incomplete".to_string());
    }
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

fn sz_decode_folder(
    folder: &SzFolder,
    packed_slices: &[&[u8]],
    password: &str,
) -> Result<Vec<u8>, String> {
    if folder.coders.is_empty() {
        return Err("7-Zip: Empty folder coders".to_string());
    }
    let encrypted = folder
        .coders
        .iter()
        .any(|c| c.id.as_slice() == [0x06, 0xf1, 0x07, 0x01]);

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

    let resolve_input = |in_idx: usize, out_bufs: &[Option<Vec<u8>>]| -> Option<Vec<u8>> {
        if let Some(&(_, out_idx)) = folder
            .bind_pairs
            .iter()
            .find(|&&(i, _)| i == in_idx as u64)
        {
            return out_bufs.get(out_idx as usize)?.clone();
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

    for _ in 0..folder.coders.len() {
        let mut progressed = false;
        for c_idx in 0..folder.coders.len() {
            if done[c_idx] {
                continue;
            }
            let c = &folder.coders[c_idx];
            let in_base = in_bases[c_idx];
            let out_base = out_bases[c_idx];
            let mut inputs = Vec::with_capacity(c.num_in);
            let mut ready = true;
            for offset in 0..c.num_in {
                if let Some(buf) = resolve_input(in_base + offset, &out_bufs) {
                    inputs.push(buf);
                } else {
                    ready = false;
                    break;
                }
            }
            if !ready {
                continue;
            }
            let unpack_sz = folder.unpack.get(out_base).copied().unwrap_or(0);
            let decoded = if c.num_in == 1 && c.num_out == 1 {
                sz_run_coder(&c.id, &c.props, &inputs[0], unpack_sz, password)
            } else if c.id.as_slice() == [0x03, 0x03, 0x01, 0x1b] && c.num_in == 4 && c.num_out == 1 {
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

    let final_idx = folder
        .final_out_stream_index()
        .ok_or_else(|| "7-Zip: Missing final output stream in folder".to_string())?;
    let buf = out_bufs
        .get_mut(final_idx)
        .and_then(Option::take)
        .ok_or_else(|| "7-Zip: Folder produced no output".to_string())?;

    if let Some(expected_crc) = folder.crc {
        if crc32(&buf) != expected_crc {
            return Err(if encrypted {
                "CRC Failed in encrypted file. Wrong password?".to_string()
            } else {
                "7-Zip: Folder CRC failed".to_string()
            });
        }
    }
    Ok(buf)
}

fn sz_read_names(data: &[u8], count: u64) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(usize::try_from(count).ok()?);
    let mut units = data.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c));
    for _ in 0..count {
        let mut name = Vec::new();
        loop {
            let unit = units.next()?;
            if unit == 0 {
                break;
            }
            name.push(unit);
        }
        out.push(String::from_utf16_lossy(&name));
    }
    Some(out)
}

fn read_7z_volumes(archive_path: &Path) -> Result<Vec<u8>, String> {
    let name = archive_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if name.to_ascii_lowercase().ends_with(".7z.001") {
        let prefix = &name[..name.len() - 3];
        let parent = archive_path.parent().unwrap_or_else(|| Path::new("."));
        let mut combined = Vec::new();
        for n in 1..1000u32 {
            let part_path = parent.join(format!("{prefix}{n:03}"));
            if !part_path.exists() {
                if n == 1 {
                    return Err(format!("7-Zip: Volume {} not found", part_path.display()));
                }
                break;
            }
            let bytes = std::fs::read(&part_path).map_err(|e| e.to_string())?;
            combined.extend_from_slice(&bytes);
        }
        Ok(combined)
    } else {
        std::fs::read(archive_path).map_err(|e| e.to_string())
    }
}

fn collect_folder_packed_slices<'a>(
    data: &'a [u8],
    streams: &SzStreams,
    folder_idx: usize,
    pack_offset: &mut u64,
    pack_idx: &mut usize,
) -> Result<Vec<&'a [u8]>, String> {
    let folder = &streams.folders[folder_idx];
    let num_pack = folder.packed_streams.len().max(1);
    let mut slices = Vec::with_capacity(num_pack);
    for _ in 0..num_pack {
        let pack_len = streams.pack_sizes.get(*pack_idx).copied().unwrap_or(0);
        *pack_idx += 1;
        let from = usize::try_from(*pack_offset).map_err(|_| "7-Zip: Pack offset overflow")?;
        let to = from
            .checked_add(usize::try_from(pack_len).map_err(|_| "7-Zip: Pack size overflow")?)
            .ok_or_else(|| "7-Zip: Pack range overflow".to_string())?;
        let Some(s) = data.get(from..to) else {
            return Err("7-Zip: Truncated packed stream".to_string());
        };
        slices.push(s);
        *pack_offset = pack_offset.saturating_add(pack_len);
    }
    Ok(slices)
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
    let data = read_7z_volumes(archive_path).map_err(|e| {
        log_cb(1, &e);
        e
    })?;
    if !data.starts_with(SEVEN_Z) || data.len() < 32 {
        let msg = "7-Zip: Not a valid 7z archive".to_string();
        log_cb(1, &msg);
        return Err(msg);
    }
    if le32(&data, 8) != Some(crc32(&data[12..32])) {
        let msg = "7-Zip: Signature header CRC mismatch".to_string();
        log_cb(1, &msg);
        return Err(msg);
    }
    let next_offset = le64(&data, 12).unwrap_or(0);
    let next_size = le64(&data, 20).unwrap_or(0);
    let next_crc = le32(&data, 28).unwrap_or(0);

    if next_size == 0 {
        log_cb(4, "7-Zip: Everything is Ok");
        return Ok(zipfast::extract::Summary::default());
    }

    let hdr_start =
        usize::try_from(32u64.saturating_add(next_offset)).map_err(|_| "Offset overflow")?;
    let hdr_end = hdr_start
        .checked_add(usize::try_from(next_size).map_err(|_| "Size overflow")?)
        .ok_or_else(|| "Header range overflow".to_string())?;
    let Some(raw_hdr) = data.get(hdr_start..hdr_end) else {
        let msg = "7-Zip: Truncated archive (missing end header)".to_string();
        log_cb(1, &msg);
        return Err(msg);
    };
    if crc32(raw_hdr) != next_crc {
        let msg = "7-Zip: End header CRC mismatch".to_string();
        log_cb(1, &msg);
        return Err(msg);
    }

    let mut hdr_bytes = raw_hdr.to_vec();
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
                let slices = collect_folder_packed_slices(
                    &data,
                    &enc_streams,
                    0,
                    &mut pack_offset,
                    &mut pack_idx,
                )
                .map_err(|e| {
                    log_cb(1, &e);
                    e
                })?;
                hdr_bytes = sz_decode_folder(&enc_streams.folders[0], &slices, password).map_err(
                    |e| {
                        log_cb(1, &e);
                        e
                    },
                )?;
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

    // Stream folders one by one on demand so only one folder's decoded buffer is held in RAM
    let mut pack_offset = 32u64.saturating_add(main_streams.pack_pos);
    let mut pack_idx = 0usize;
    let mut cur_folder_idx = 0usize;
    let mut cur_folder_rem_sub = 0usize;
    let mut cur_folder_sub_pos = 0usize;
    let mut cur_folder_buf: Vec<u8> = Vec::new();

    let mut summary = zipfast::extract::Summary::default();
    let mut stream_cursor = 0usize;
    let mut empty_cursor = 0usize;
    for (i, name) in file_names.iter().enumerate() {
        let attr = win_attrs.get(i).copied().flatten().unwrap_or(0);
        let unix_mode = attr >> 16;
        if (unix_mode & 0o170000) == 0o120000 || (attr & 0x0400) != 0 {
            log_cb(1, "ERROR: Dangerous link path was ignored");
            log_cb(0, "Stream repair: archive contains link");
            return Err("archive contains link".to_string());
        }
        let rel = zipfast::path::sanitize(name, true).map_err(|e| {
            let msg = format!("7-Zip: {e}");
            log_cb(1, &msg);
            msg
        })?;
        let is_empty_stream = empty_stream.get(i).copied().unwrap_or(false);
        if is_empty_stream {
            let is_empty_file = empty_file.get(empty_cursor).copied().unwrap_or(false);
            empty_cursor += 1;
            if !is_empty_file {
                // Directory
                let dir_path = output_dir.join(&rel);
                std::fs::create_dir_all(&dir_path).map_err(|e| e.to_string())?;
                summary.dirs += 1;
                continue;
            }
            log_cb(4, &format!("7-Zip: Extracting  {name}"));
            if let Some(dest_path) = resolve_dest_path(output_dir, &rel, overwrite_mode) {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::File::create(&dest_path).map_err(|e| e.to_string())?;
                if unix_mode != 0 {
                    apply_unix_executable_mode(&dest_path, unix_mode);
                }
                summary.files += 1;
            } else {
                summary.skipped += 1;
            }
        } else {
            while cur_folder_rem_sub == 0 {
                if cur_folder_idx >= main_streams.folders.len() {
                    let msg = "7-Zip: Missing folder stream for file".to_string();
                    log_cb(1, &msg);
                    return Err(msg);
                }
                let f_idx = cur_folder_idx;
                cur_folder_idx += 1;
                let n_sub = main_streams.per_folder.get(f_idx).copied().unwrap_or(1) as usize;
                let slices = collect_folder_packed_slices(
                    &data,
                    &main_streams,
                    f_idx,
                    &mut pack_offset,
                    &mut pack_idx,
                )
                .map_err(|e| {
                    log_cb(1, &e);
                    e
                })?;
                if n_sub == 0 {
                    continue;
                }
                cur_folder_buf = sz_decode_folder(&main_streams.folders[f_idx], &slices, password)
                    .map_err(|e| {
                        log_cb(1, &e);
                        e
                    })?;
                cur_folder_rem_sub = n_sub;
                cur_folder_sub_pos = 0;
            }

            let sz_u64 = main_streams
                .sizes
                .get(stream_cursor)
                .copied()
                .ok_or_else(|| "7-Zip: Missing substream size for file".to_string())?;
            let sz = usize::try_from(sz_u64).map_err(|_| "7-Zip: Substream size overflow")?;
            let expected_crc = main_streams.digests.get(stream_cursor).copied().flatten();
            stream_cursor += 1;
            cur_folder_rem_sub -= 1;

            let end = cur_folder_sub_pos
                .checked_add(sz)
                .ok_or_else(|| "7-Zip: Substream offset overflow".to_string())?;
            let Some(payload) = cur_folder_buf.get(cur_folder_sub_pos..end) else {
                let msg = "7-Zip: Decoded folder shorter than file substream size".to_string();
                log_cb(1, &msg);
                return Err(msg);
            };
            cur_folder_sub_pos = end;

            log_cb(4, &format!("7-Zip: Extracting  {name}"));
            if let Some(crc) = expected_crc {
                if crc32(payload) != crc {
                    let msg = format!("7-Zip: {name} - CRC failed");
                    log_cb(1, &msg);
                    return Err(msg);
                }
            }
            if let Some(dest_path) = resolve_dest_path(output_dir, &rel, overwrite_mode) {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let mut f = std::fs::File::create(&dest_path).map_err(|e| e.to_string())?;
                f.write_all(payload).map_err(|e| e.to_string())?;
                drop(f);
                if unix_mode != 0 {
                    apply_unix_executable_mode(&dest_path, unix_mode);
                }
                summary.files += 1;
                summary.bytes += payload.len() as u64;
            } else {
                summary.skipped += 1;
            }
            if cur_folder_rem_sub == 0 {
                cur_folder_buf = Vec::new();
            }
        }
    }

    log_cb(4, "7-Zip: Everything is Ok");
    Ok(summary)
}
