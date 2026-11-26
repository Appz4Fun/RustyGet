//! Restarting a compressed backup's decoder near the frontier (failover-continue for alts whose
//! video is compressed: RAR5 methods 1-5 and 7z LZMA2).
//!
//! A stored backup lends the video's bytes by range through its payload map
//! (`streamrepair::ContentMapper::build_map_with_tls`). A compressed one has no byte of the video
//! at a fixed place, so before zipfast #290 it could only lend after a whole download and
//! extraction. zipfast restarts its decoder inside the member instead (`map::open_at`, then
//! `complete_rar5` or `complete_lzma2`): it plans which packed bytes it needs from shortly before
//! the frontier's packed position, and the download's own output before the frontier fills the
//! decoder's window (RAR5 `Seed::HeldPlaintext`) or dictionary (LZMA2). Only the packed bytes
//! from the restart to the member's end are read from the backup, each once; nothing before the
//! restart is downloaded or decoded (for LZMA2 the chunk headers before it are walked, see
//! [`restart`]).
//!
//! The decoded output, from the restart to the member's end, is kept in a scratch file and handed
//! to stream repair as one more layer of the backup: a bare file mapped by `build_map_with_tls`,
//! readable from the restart on. So the bytes lent are still the video's own, matched against the
//! download's bytes before the frontier (the output decoded again between the restart and the
//! frontier is compared), and never container bytes. The whole member is proven by its CRC32
//! before anything is lent: the download's own output before the restart and the decoded rest
//! together must match the CRC the backup's headers record.
//!
//! What this does not restart, it says why (the failover then starts the backup from its first
//! byte, as before): another coder (RAR 4.x compressed, ZIP Deflate, 7z BZip2/PPMd/... restart
//! only at their start), 7zAES under LZMA2 (RAR5 AES under compression is decrypted on the
//! way, its key derived once), a later member of a RAR solid
//! stream, a member with no CRC32 to prove the output, and an LZMA2 rest larger than
//! [`LZMA2_MAX_REST`] (zipfast decodes it in one buffer).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use zipfast::map::{
    complete_lzma2, complete_rar5, member_restart, open_at, Layer, Lzma2DecodeOptions,
    Lzma2IndexBuilder, Lzma2IndexOptions, MapBuilder, MapOptions, MemberMap, OpenAt, OpenAtError,
    OpenAtOptions, Seed, Start,
};
use zipfast::progressive::{
    ProgressiveResult, RangeRead, RangeSource, SourceId, SourceInfo, SourceLength, VolumeAnswer,
    VolumeRequest, VolumeSource,
};

/// Output decoded again before the frontier at the least: the restart is planned this far before
/// it, so the bytes stream repair compares (its probe windows hug the hole) are decoded.
const OVERLAP: u64 = 256 << 10;
/// The packed bytes searched for a RAR5 restart before the frontier's estimated packed position
/// on a first try (zipfast `SEED_SEARCH`). A seeded restart found in them may be unguarded; the
/// member's CRC32 proves the output either way. When none is found the search widens once to
/// zipfast's default (a guarded restart's reach).
const RAR5_FIRST_SEARCH: u64 = zipfast::map::SEED_SEARCH;
/// The most output of the download held in memory as the restart's window (RAR5 asks for its
/// dictionary and a filter's reach, twice; LZMA2 for its dictionary).
const MAX_KNOWN: u64 = 1 << 30;
/// The most packed and decoded bytes an LZMA2 restart holds at once: zipfast decodes the rest of
/// the folder in one buffer from packed bytes held in one buffer.
pub const LZMA2_MAX_REST: u64 = 768 << 20;
/// Bytes read from the backup per call while the decoder is fed.
const FEED_CHUNK: usize = 1 << 20;
/// The most rounds of reads one map or index walk may ask for.
const MAX_ROUNDS: usize = 1 << 20;

/// What a restart decoded: member output `from..size` of the backup's media member, in a scratch
/// file removed when this is dropped.
#[derive(Debug)]
pub struct Restarted {
    file: Mutex<File>,
    path: PathBuf,
    /// The member output offset of the first decoded byte (the restart, at or before the frontier).
    pub from: u64,
    /// The member's size.
    pub size: u64,
    /// Packed bytes read from the backup (headers for the map and walk included).
    pub fetched: u64,
    /// "RAR5" or "LZMA2".
    pub layer: &'static str,
    /// Whether decoding alone proved the window (RAR5 `Rar5Resume::checked`); the CRC32 proved it either way.
    pub window_checked: bool,
    /// From the first read of the backup to the CRC32 proven.
    pub elapsed: Duration,
}

impl Restarted {
    /// Reads decoded output at member offset `offset`: all of `buf` or false (before `from`, past
    /// the end, or a read error).
    pub fn read(&self, offset: u64, buf: &mut [u8]) -> bool {
        use std::io::Read;
        if offset < self.from
            || offset
                .checked_add(buf.len() as u64)
                .is_none_or(|end| end > self.size)
        {
            return false;
        }
        let mut f = self.file.lock().unwrap_or_else(|e| e.into_inner());
        f.seek(SeekFrom::Start(offset - self.from)).is_ok() && f.read_exact(buf).is_ok()
    }
}

impl Drop for Restarted {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// What was found restarting a backup's set for a download, by (download NZBID, key): a
/// restart is tried once per download and backup set, and its output kept for every later borrow
/// run (each dead volume found later is lent from it, nothing read again).
type Tried = std::collections::HashMap<(i32, Vec<u8>), Result<Arc<Restarted>, String>>;

fn tried() -> std::sync::MutexGuard<'static, Tried> {
    static TRIED: std::sync::OnceLock<Mutex<Tried>> = std::sync::OnceLock::new();
    TRIED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// The key of a backup's set: its queued nzb file and the set's first volume.
pub fn set_key(donor_queued_filename: &[u8], first_volume: &[u8]) -> Vec<u8> {
    let mut key = donor_queued_filename.to_vec();
    key.push(b'|');
    key.extend_from_slice(first_volume);
    key
}

/// The restart tried earlier for download `nzb_id` and backup set `key`: its output, or why it
/// could not be restarted.
pub fn earlier(nzb_id: i32, key: &[u8]) -> Option<Result<Arc<Restarted>, String>> {
    tried().get(&(nzb_id, key.to_vec())).cloned()
}

/// Keeps what restarting backup set `key` for download `nzb_id` gave.
pub fn keep(nzb_id: i32, key: &[u8], result: Result<Arc<Restarted>, String>) {
    tried().insert((nzb_id, key.to_vec()), result);
}

/// Why no set of the backup whose queued nzb file is `donor` could be restarted for download
/// `nzb_id`, when one was tried and none could.
pub fn refusal(nzb_id: i32, donor: &[u8]) -> Option<String> {
    let tried = tried();
    let mut prefix = donor.to_vec();
    prefix.push(b'|');
    let mine = tried
        .iter()
        .filter(|((id, k), _)| *id == nzb_id && k.starts_with(&prefix));
    let mut why = None;
    for (_, r) in mine {
        match r {
            Ok(_) => return None,
            Err(e) => why = Some(e.clone()),
        }
    }
    why
}

/// Forgets the restarts of download `nzb_id` (their scratch files go with the last holder).
pub fn forget(nzb_id: i32) {
    tried().retain(|(id, _), _| *id != nzb_id);
}

/// The backup as the restart reads it: its volumes in set order, read by volume offset.
pub struct Backup<'a> {
    /// The first volume's name (zipfast tells the set kind from it: `name.7z.001`, `name.part1.rar`).
    pub first_name: &'a [u8],
    /// Every volume's length, in set order.
    pub sizes: &'a [u64],
    /// The password its nzb carries, if any.
    pub password: Option<&'a str>,
    /// Reads all of `buf` from volume `volume` at `offset`, or false.
    pub read: &'a mut dyn FnMut(u32, u64, &mut [u8]) -> bool,
    /// Says which `(volume, offset, len)` ranges are read next, so they are fetched as one batch.
    pub prefetch: &'a mut dyn FnMut(&[(u32, u64, u64)]),
}

/// The packed bytes fetched ahead of the decoder in one batch.
const PREFETCH_WINDOW: u64 = 16 << 20;

/// Restarts the backup's member holding `size` bytes (the download's media payload) so that it
/// decodes from at or before `frontier` (the download's first missing output byte) to its end.
/// `known` reads the download's own output before the frontier: all of `buf` at `offset`, or
/// false. The decoded output goes to a scratch file in `scratch_dir`.
///
/// `Err` says why the backup cannot be restarted there, in words for the log.
pub fn restart(
    backup: &mut Backup<'_>,
    size: u64,
    frontier: u64,
    known: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    scratch_dir: &Path,
) -> Result<Restarted, String> {
    let started = Instant::now();
    let reads = Reads::new(backup.sizes.to_vec());
    let map = build_map(backup, &reads)?;
    let index = map
        .members
        .iter()
        .position(|m| !m.is_dir && m.unpacked_size == Some(size))
        .ok_or_else(|| "it holds no member of the video's size".to_string())?;
    let restart = member_restart(&map, index).map_err(|e| e.to_string())?;
    match restart.layer {
        Layer::Rar5Compressed | Layer::SevenZipLzma2 => {}
        Layer::Stored | Layer::StoredCbc | Layer::StoredCounter => {
            return Err("its member is stored: it lends by range, not by a restart".into());
        }
        other => {
            return Err(format!(
                "a {other:?} member restarts only at its start (no restart point inside it)"
            ))
        }
    }
    let member = &map.members[index];
    // Under RAR5 AES the packed bytes are decrypted on the way (the key is derived once, from
    // the member's salt and the password its nzb carries); 7zAES under LZMA2 is not restarted.
    let cipher = if restart.encrypted {
        match (&member.cipher, backup.password) {
            (
                Some(zipfast::map::CipherParams::Rar5 {
                    kdf_count,
                    salt,
                    iv: Some(iv),
                    check,
                    ..
                }),
                Some(pw),
            ) => {
                let (key, hash_key) =
                    zipfast::crypto::derive_rar5_key_and_hash_key(pw, *kdf_count, salt, *check)
                        .ok_or_else(|| {
                            "its password does not open its compressed member".to_string()
                        })?;
                Some(Cbc { key, hash_key, iv: *iv })
            }
            (Some(zipfast::map::CipherParams::Rar5 { .. }), None) => {
                return Err(
                    "its compressed member is encrypted and its nzb carries no password".into(),
                );
            }
            _ => {
                return Err(
                    "its compressed member is encrypted with a cipher a restart does not decrypt (7zAES)"
                        .into(),
                )
            }
        }
    } else {
        None
    };
    let keyed = member.checks.as_ref().is_some_and(|c| c.keyed);
    if keyed && cipher.is_none() {
        return Err("its member's CRC32 is keyed with a password it was not opened with".into());
    }
    let want_crc = member
        .checks
        .as_ref()
        .and_then(|c| c.crc32)
        .or_else(|| {
            member
                .folder
                .and_then(|slot| map.folders.get(slot.index as usize))
                .filter(|f| f.members == 1)
                .and_then(|f| f.crc32)
        })
        .ok_or_else(|| "its member records no CRC32 to prove the decoded output".to_string())?;
    if frontier == 0 || frontier >= size {
        return Err("nothing before the frontier to restart from".into());
    }
    let target = frontier.saturating_sub(OVERLAP);
    let scratch = scratch_path(scratch_dir);
    let mut out =
        OutputFile::create(&scratch).map_err(|e| format!("cannot create its scratch file: {e}"))?;
    let (from, window_checked) = match restart.layer {
        Layer::Rar5Compressed => rar5(
            backup,
            &reads,
            &map,
            index,
            target,
            cipher.as_ref(),
            known,
            &mut out,
        )?,
        _ => lzma2(
            backup, &reads, &map, index, target, frontier, known, &mut out,
        )?,
    };
    let decoded_crc = out
        .finish(size - from)
        .map_err(|e| format!("its scratch file failed: {e}"))?;
    // The member's CRC32: the download's own output before the restart, then the decoded rest.
    let prefix_crc = crc_of_known(known, from)
        .ok_or_else(|| "the download's own output could not be read".to_string())?;
    let crc = zipfast::crc32::combine(prefix_crc, decoded_crc, size - from);
    // a RAR5 AES member keeps its CRC32 keyed with the password's hash key
    let crc = match (&cipher, keyed) {
        (Some(c), true) => zipfast::crypto::rar5_tweak_crc32(crc, &c.hash_key),
        _ => crc,
    };
    if crc != want_crc {
        return Err("the output decoded from the restart failed the member's CRC32".into());
    }
    let fetched = reads.fetched();
    Ok(Restarted {
        file: Mutex::new(out.into_file()),
        path: scratch,
        from,
        size,
        fetched,
        layer: if restart.layer == Layer::Rar5Compressed {
            "RAR5"
        } else {
            "LZMA2"
        },
        window_checked,
        elapsed: started.elapsed(),
    })
}

fn scratch_path(dir: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".rg-restart-{}-{n}.out", std::process::id()))
}

/// The CRC32 of the download's output `0..len`, read in pieces.
fn crc_of_known(known: &mut dyn FnMut(u64, &mut [u8]) -> bool, len: u64) -> Option<u32> {
    let mut hasher = zipfast::crc32::Hasher::new();
    let mut buf = vec![0u8; FEED_CHUNK.min(len as usize).max(1)];
    let mut at = 0u64;
    while at < len {
        let n = ((len - at) as usize).min(buf.len());
        if !known(at, &mut buf[..n]) {
            return None;
        }
        hasher.update(&buf[..n]);
        at += n as u64;
    }
    Some(hasher.finalize())
}

/// The decoded output as it is written: in order, hashed on the way.
struct OutputFile {
    writer: std::io::BufWriter<File>,
    hasher: zipfast::crc32::Hasher,
    written: u64,
}

impl OutputFile {
    fn create(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        Ok(Self {
            writer: std::io::BufWriter::with_capacity(1 << 20, file),
            hasher: zipfast::crc32::Hasher::new(),
            written: 0,
        })
    }

    fn put(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(bytes)?;
        self.hasher.update(bytes);
        self.written += bytes.len() as u64;
        Ok(())
    }

    /// Flushes and checks that exactly `want` bytes were written; gives their CRC32.
    fn finish(&mut self, want: u64) -> std::io::Result<u32> {
        self.writer.flush()?;
        if self.written != want {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "decoded {} bytes, the rest of the member is {want}",
                    self.written
                ),
            ));
        }
        Ok(self.hasher.clone().finalize())
    }

    fn into_file(self) -> File {
        // `finish` flushed it: nothing is left to write
        let (file, _) = self.writer.into_parts();
        file
    }
}

/// Reads one member stream offset range of the backup's packed bytes through `spans`.
fn read_stream(
    backup: &mut Backup<'_>,
    reads: &Reads,
    spans: &[zipfast::map::PackedSpan],
    from: u64,
    buf: &mut [u8],
) -> Result<(), String> {
    // a long read goes window by window, each fetched as one batch first
    if buf.len() as u64 > PREFETCH_WINDOW {
        let mut at = 0usize;
        while at < buf.len() {
            let n = (buf.len() - at).min(PREFETCH_WINDOW as usize);
            prefetch_stream(backup, spans, from + at as u64, n as u64);
            read_stream(backup, reads, spans, from + at as u64, &mut buf[at..at + n])?;
            at += n;
        }
        return Ok(());
    }
    let end = from + buf.len() as u64;
    let mut filled = 0u64;
    for s in spans {
        let (a, b) = (s.stream.start.max(from), s.stream.end.min(end));
        if a >= b {
            continue;
        }
        let at = s.packed.start + (a - s.stream.start);
        let dst = &mut buf[(a - from) as usize..(b - from) as usize];
        if !reads.read_through(backup, s.volume, at, dst) {
            return Err(format!(
                "its volume {} could not be read at {at}",
                s.volume + 1
            ));
        }
        filled += b - a;
    }
    if filled != buf.len() as u64 {
        return Err("its map lacks packed bytes the restart needs".into());
    }
    Ok(())
}

/// Asks for member stream bytes `from..from + len` as one batch.
fn prefetch_stream(
    backup: &mut Backup<'_>,
    spans: &[zipfast::map::PackedSpan],
    from: u64,
    len: u64,
) {
    let end = from + len;
    let ranges: Vec<(u32, u64, u64)> = spans
        .iter()
        .filter_map(|s| {
            let (a, b) = (s.stream.start.max(from), s.stream.end.min(end));
            (a < b).then(|| (s.volume, s.packed.start + (a - s.stream.start), b - a))
        })
        .collect();
    if !ranges.is_empty() {
        (backup.prefetch)(&ranges);
    }
}

/// The download's own output `range`, for a restart's window.
fn read_known(
    known: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    from: u64,
    to: u64,
) -> Result<Vec<u8>, String> {
    if to - from > MAX_KNOWN {
        return Err(format!(
            "its window needs {} MB of the download's output, more than is held for it",
            (to - from) >> 20
        ));
    }
    let mut buf = vec![0u8; (to - from) as usize];
    if !known(from, &mut buf) {
        return Err("the download's own output before the frontier could not be read".into());
    }
    Ok(buf)
}

/// A RAR5 AES member's data key and IV, and the hash key its CRC32 is keyed with.
struct Cbc {
    key: [u8; 32],
    hash_key: [u8; 32],
    iv: [u8; 16],
}

/// Decrypts a member's AES-CBC packed stream as it is read: whole blocks, each with the
/// ciphertext block before it as its IV.
struct CbcStream<'k> {
    key: &'k [u8; 32],
    iv: [u8; 16],
}

impl CbcStream<'_> {
    /// Decrypts `ct` (whole blocks) and keeps its last block as the next IV.
    fn decrypt(&mut self, ct: &[u8]) -> Result<Vec<u8>, String> {
        let plain = zipfast::crypto::decrypt_cbc(self.key, &self.iv, ct)
            .ok_or_else(|| "its key is not an AES key".to_string())?;
        if let Some(last) = ct.len().checked_sub(16).and_then(|at| ct.get(at..)) {
            self.iv.copy_from_slice(last);
        }
        Ok(plain)
    }
}

#[allow(clippy::too_many_arguments)]
fn rar5(
    backup: &mut Backup<'_>,
    reads: &Reads,
    map: &MemberMap,
    index: usize,
    target: u64,
    cipher: Option<&Cbc>,
    known: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    out: &mut OutputFile,
) -> Result<(u64, bool), String> {
    use zipfast::rar5::unpack::NearOptions;
    let member = &map.members[index];
    let mut search = Some(RAR5_FIRST_SEARCH);
    let mut held: Option<(u64, Vec<u8>)> = None;
    loop {
        let options = OpenAtOptions {
            seed: Seed::HeldPlaintext { strict: false },
            search,
            ..Default::default()
        };
        let plan = open_at(map, index, target, &options).map_err(|e| e.to_string())?;
        let Start::Rar5 { first, donor, .. } = plan.start.clone() else {
            return Err("the restart plan is not a RAR5 one".into());
        };
        if plan.restart.stream_start != 0 {
            return Err("its member is a later member of a solid stream".into());
        }
        // The packed bytes searched: from the plan's start to `first.end`, adding to what an
        // earlier, narrower try already read (never read twice). Under AES they end on a
        // block and are decrypted: the block before the first one fetched is its IV (at the
        // stream's start the member's IV is).
        let first_end = if cipher.is_some() {
            first.end & !15
        } else {
            first.end
        };
        extend_held(backup, reads, &plan, &mut held, first_end)?;
        let (raw_from, raw) = held
            .as_ref()
            .map(|(f, b)| (*f, b.as_slice()))
            .expect("just read");
        let mut stream = cipher.map(|c| CbcStream {
            key: &c.key,
            iv: c.iv,
        });
        let decrypted;
        let (packed_from, bytes) = match stream.as_mut() {
            None => (raw_from, raw),
            Some(cbc) => {
                let skip = if raw_from == 0 { 0 } else { 16 };
                if raw.len() < skip || raw_from % 16 != 0 {
                    return Err("its encrypted packed bytes are not block aligned".into());
                }
                if skip == 16 {
                    cbc.iv.copy_from_slice(&raw[..16]);
                }
                decrypted = cbc.decrypt(&raw[skip..])?;
                (raw_from + skip as u64, decrypted.as_slice())
            }
        };
        let known_from = donor.start.min(target);
        let window = read_known(known, known_from, target)?;
        let near = NearOptions {
            deadline: Some(Duration::from_secs(20)),
            ..NearOptions::default()
        };
        match complete_rar5(map, &plan, packed_from, bytes, known_from, &window, &near) {
            Ok(mut resume) => {
                drop(window);
                let mut feed = |decoder: &mut zipfast::rar5::unpack::Unpacker,
                                plain: &[u8]|
                 -> Result<(), String> {
                    let mut sink = |b: &[u8]| -> zipfast::error::Result<()> {
                        out.put(b).map_err(zipfast::error::Error::from)
                    };
                    decoder
                        .feed(plain, &mut sink)
                        .map_err(|e| format!("decoding failed: {e}"))
                };
                // Feed from the restart: what is held first, then the rest of the member.
                let skip = (resume.feed_from - packed_from) as usize;
                feed(&mut resume.decoder, &bytes[skip..])?;
                let mut at = raw_from + raw.len() as u64;
                let mut chunk = vec![0u8; FEED_CHUNK];
                let mut fetched_to = at;
                while at < member.packed_size {
                    if at >= fetched_to {
                        let len = (member.packed_size - at).min(PREFETCH_WINDOW);
                        prefetch_stream(backup, &plan.spans, at, len);
                        fetched_to = at + len;
                    }
                    let n = ((member.packed_size - at) as usize).min(chunk.len());
                    read_stream(backup, reads, &plan.spans, at, &mut chunk[..n])?;
                    match stream.as_mut() {
                        None => feed(&mut resume.decoder, &chunk[..n])?,
                        Some(cbc) => {
                            let plain = cbc.decrypt(&chunk[..n & !15])?;
                            feed(&mut resume.decoder, &plain)?;
                        }
                    }
                    at += n as u64;
                }
                let mut sink = |b: &[u8]| -> zipfast::error::Result<()> {
                    out.put(b).map_err(zipfast::error::Error::from)
                };
                resume
                    .decoder
                    .finish(&mut sink)
                    .map_err(|e| format!("decoding failed: {e}"))?;
                return Ok((resume.output, resume.checked));
            }
            Err(OpenAtError::Resync(_)) if search.is_some() => {
                // No restart in the bytes searched: widen once to a guarded restart's reach.
                search = None;
            }
            Err(e) => return Err(format!("no restart: {e}")),
        }
    }
}

/// The member stream bytes from `plan.fetch_from` to `end`, reusing `held` (bytes an earlier try
/// read from a later offset to the same or a smaller end).
fn extend_held(
    backup: &mut Backup<'_>,
    reads: &Reads,
    plan: &OpenAt,
    held: &mut Option<(u64, Vec<u8>)>,
    end: u64,
) -> Result<(), String> {
    let from = plan.fetch_from.min(end);
    let mut bytes = Vec::new();
    match held.take() {
        Some((h_from, h)) if h_from >= from && h_from + h.len() as u64 <= end => {
            let mut before = vec![0u8; (h_from - from) as usize];
            read_stream(backup, reads, &plan.spans, from, &mut before)?;
            let h_end = h_from + h.len() as u64;
            let mut after = vec![0u8; (end - h_end) as usize];
            read_stream(backup, reads, &plan.spans, h_end, &mut after)?;
            bytes.extend_from_slice(&before);
            bytes.extend_from_slice(&h);
            bytes.extend_from_slice(&after);
        }
        _ => {
            bytes = vec![0u8; (end - from) as usize];
            read_stream(backup, reads, &plan.spans, from, &mut bytes)?;
        }
    }
    *held = Some((from, bytes));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lzma2(
    backup: &mut Backup<'_>,
    reads: &Reads,
    map: &MemberMap,
    index: usize,
    target: u64,
    frontier: u64,
    known: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    out: &mut OutputFile,
) -> Result<(u64, bool), String> {
    let member = &map.members[index];
    let slot = member
        .folder
        .ok_or_else(|| "its member has no folder".to_string())?;
    let folder = map
        .folders
        .get(slot.index as usize)
        .ok_or_else(|| "its folder is not in the map".to_string())?;
    let plan = open_at(map, index, target, &OpenAtOptions::default()).map_err(|e| e.to_string())?;
    let Start::Lzma2 {
        folder_target,
        donor,
        dict_prop: _,
        ..
    } = plan.start.clone()
    else {
        return Err("the restart plan is not an LZMA2 one".into());
    };
    let packed_len: u64 = folder.pack_sizes.iter().sum();
    let rest = (packed_len - plan.fetch_from) + (folder.unpacked_size - folder_target);
    if rest > LZMA2_MAX_REST {
        return Err(format!(
            "the rest of its LZMA2 folder ({} MB packed and unpacked) is more than a restart holds",
            rest >> 20
        ));
    }
    // The chunk framing, walked from the folder's start up to the bytes fetched for the restart
    // (every chunk header before them; a walk is the only proof of where chunks start).
    let limit = stream_to_volume(&plan.spans, plan.fetch_from);
    reads.set_limit(limit);
    let first_name =
        std::ffi::OsString::from(String::from_utf8_lossy(backup.first_name).into_owned());
    let mut builder = Lzma2IndexBuilder::for_folder(
        Box::new(reads.volumes()),
        folder,
        first_name,
        Lzma2IndexOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    let index_walked =
        drive(backup, reads, |cx| builder.poll_index(cx))?.map_err(|e| e.to_string())?;
    reads.set_limit(None);
    let mut packed_from = plan.fetch_from;
    let mut packed = vec![0u8; (packed_len - packed_from) as usize];
    read_stream(backup, reads, &plan.spans, packed_from, &mut packed)?;
    // Folder output before the target as the dictionary: what the plan asks for, from the
    // member's start on (the download holds the member's output, not other members').
    let member_floor = slot.offset;
    let mut known_from = donor.start.max(member_floor);
    let mut tries = 0;
    loop {
        tries += 1;
        let window = read_known(known, known_from - member_floor, target)?;
        let options = Lzma2DecodeOptions {
            max_output: LZMA2_MAX_REST,
            ..Lzma2DecodeOptions::default()
        };
        match complete_lzma2(
            map,
            &plan,
            packed_from,
            &packed,
            known_from,
            &window,
            Some(&index_walked),
            &options,
        ) {
            Ok(resume) => {
                // The folder's output from `resume.output`: this member's part of it.
                let member_end = slot.offset + member.unpacked_size.unwrap_or(0);
                if resume.output < member_floor {
                    return Err("the restart lies before the member's start".into());
                }
                let take = (member_end - resume.output) as usize;
                let bytes = resume
                    .bytes
                    .get(..take)
                    .ok_or_else(|| "the restart decoded too little".to_string())?;
                out.put(bytes)
                    .map_err(|e| format!("its scratch file failed: {e}"))?;
                let _ = frontier;
                return Ok((resume.output - member_floor, true));
            }
            Err(OpenAtError::FetchFrom {
                packed: p,
                output: o,
            }) if tries < 4 && p < packed_from => {
                // The nearest real restart lies before the bytes held: read from there too.
                let mut before = vec![0u8; (packed_from - p) as usize];
                read_stream(backup, reads, &plan.spans, p, &mut before)?;
                before.extend_from_slice(&packed);
                packed = before;
                packed_from = p;
                let dict = plan.restart.dictionary.unwrap_or(0);
                known_from = o.saturating_sub(dict).max(member_floor).min(known_from);
            }
            Err(OpenAtError::NeedMore { needed })
                if tries < 4 && needed.start < known_from && needed.start >= member_floor =>
            {
                known_from = needed.start;
            }
            Err(e) => return Err(format!("no restart: {e}")),
        }
    }
}

/// Where member stream offset `at` lies in the volumes: `(volume, offset)`.
fn stream_to_volume(spans: &[zipfast::map::PackedSpan], at: u64) -> Option<(u32, u64)> {
    spans
        .iter()
        .find(|s| s.stream.start <= at && at < s.stream.end)
        .map(|s| (s.volume, s.packed.start + (at - s.stream.start)))
}

/// Builds the backup's member map from its header bytes.
fn build_map(backup: &mut Backup<'_>, reads: &Reads) -> Result<MemberMap, String> {
    let first_name =
        std::ffi::OsString::from(String::from_utf8_lossy(backup.first_name).into_owned());
    let mut builder = MapBuilder::new(
        Box::new(reads.volumes()),
        MapOptions::new(first_name).password(backup.password),
    );
    drive(backup, reads, |cx| builder.poll_map(cx))?
        .map_err(|e| format!("its headers could not be mapped: {e}"))
}

/// Polls `poll` to completion, reading what each pending read asks for in between.
fn drive<T>(
    backup: &mut Backup<'_>,
    reads: &Reads,
    mut poll: impl FnMut(&mut Context<'_>) -> Poll<T>,
) -> Result<T, String> {
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..MAX_ROUNDS {
        if let Poll::Ready(v) = poll(&mut cx) {
            return Ok(v);
        }
        let Some((volume, offset, len)) = reads.take_wanted() else {
            return Err("its headers asked for nothing it could read".into());
        };
        reads.fetch(backup, volume, offset, len);
    }
    Err("its headers needed too many reads".into())
}

/// Bytes read from the backup, shared with the zipfast sources the map and the walk read through.
#[derive(Clone)]
struct Reads(Arc<Mutex<ReadsInner>>);

struct ReadsInner {
    sizes: Vec<u64>,
    /// Per volume: bytes read, by offset.
    held: Vec<BTreeMap<u64, Vec<u8>>>,
    /// Per volume: ranges found unreadable.
    holes: Vec<Vec<(u64, u64)>>,
    wanted: Option<(u32, u64, usize)>,
    fetched: u64,
    /// Reads at or past this volume position answer as a hole (an index walk stops there).
    limit: Option<(u32, u64)>,
}

/// The least read when a header asks for a few bytes: one read then serves the next headers too.
const READ_AHEAD: usize = 64 << 10;

impl Reads {
    fn new(sizes: Vec<u64>) -> Self {
        let n = sizes.len();
        Self(Arc::new(Mutex::new(ReadsInner {
            sizes,
            held: vec![BTreeMap::new(); n],
            holes: vec![Vec::new(); n],
            wanted: None,
            fetched: 0,
            limit: None,
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ReadsInner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn volumes(&self) -> BackupVolumes {
        BackupVolumes {
            reads: self.clone(),
        }
    }

    fn set_limit(&self, limit: Option<(u32, u64)>) {
        self.lock().limit = limit;
    }

    fn count(&self, n: u64) {
        self.lock().fetched += n;
    }

    fn fetched(&self) -> u64 {
        self.lock().fetched
    }

    fn take_wanted(&self) -> Option<(u32, u64, usize)> {
        self.lock().wanted.take()
    }

    /// Reads what a header asked for (at least [`READ_AHEAD`], within the volume).
    fn fetch(&self, backup: &mut Backup<'_>, volume: u32, offset: u64, len: usize) {
        let vol_len = self.lock().sizes.get(volume as usize).copied().unwrap_or(0);
        let mut n = len.max(READ_AHEAD) as u64;
        if let Some((lv, lo)) = self.lock().limit {
            if lv == volume && lo > offset {
                n = n.min(lo - offset);
            }
        }
        // up to the next bytes already held: nothing is read twice
        if let Some((&next, _)) = self
            .lock()
            .held
            .get(volume as usize)
            .and_then(|h| h.range(offset + 1..).next())
        {
            n = n.min(next - offset);
        }
        let n = n.min(vol_len.saturating_sub(offset)).max(1) as usize;
        let mut buf = vec![0u8; n];
        let ok = offset < vol_len && (backup.read)(volume, offset, &mut buf);
        let mut inner = self.lock();
        if ok {
            inner.fetched += n as u64;
            inner.held[volume as usize].insert(offset, buf);
        } else if let Some(h) = inner.holes.get_mut(volume as usize) {
            h.push((offset, offset + len.max(1) as u64));
        }
    }

    /// Reads all of `buf` from volume `volume` at `offset`: what the map or the walk read already
    /// is copied, the rest read from the backup (and not kept). False when a read fails.
    fn read_through(
        &self,
        backup: &mut Backup<'_>,
        volume: u32,
        offset: u64,
        buf: &mut [u8],
    ) -> bool {
        let mut at = 0usize;
        while at < buf.len() {
            let pos = offset + at as u64;
            let (copied, gap_end) = {
                let inner = self.lock();
                let Some(held) = inner.held.get(volume as usize) else {
                    return false;
                };
                match held
                    .range(..=pos)
                    .rev()
                    .find(|(o, b)| pos < **o + b.len() as u64)
                {
                    Some((&o, b)) => {
                        let from = (pos - o) as usize;
                        let n = (b.len() - from).min(buf.len() - at);
                        buf[at..at + n].copy_from_slice(&b[from..from + n]);
                        (n, 0)
                    }
                    None => {
                        let next = held.range(pos + 1..).next().map_or(u64::MAX, |(&o, _)| o);
                        (0, next.min(offset + buf.len() as u64))
                    }
                }
            };
            if copied > 0 {
                at += copied;
                continue;
            }
            let n = (gap_end - pos) as usize;
            if !(backup.read)(volume, pos, &mut buf[at..at + n]) {
                return false;
            }
            self.count(n as u64);
            at += n;
        }
        true
    }

    /// Answers a read of volume `volume` at `offset` from what is held.
    fn read_at(&self, volume: u32, offset: u64, buf: &mut [u8]) -> RangeRead {
        let mut inner = self.lock();
        let past_limit = inner
            .limit
            .is_some_and(|(lv, lo)| volume > lv || (volume == lv && offset >= lo));
        if past_limit {
            return RangeRead::Hole {
                len: buf.len() as u64,
            };
        }
        let v = volume as usize;
        // the chunk holding `offset` (a later, shorter one may start after an earlier one that holds it)
        let holding = inner.held[v]
            .range(..=offset)
            .rev()
            .find(|(at, b)| offset < **at + b.len() as u64);
        if let Some((&at, bytes)) = holding {
            let from = (offset - at) as usize;
            let n = (bytes.len() - from).min(buf.len());
            buf[..n].copy_from_slice(&bytes[from..from + n]);
            return RangeRead::Filled { count: n };
        }
        if inner.holes[v]
            .iter()
            .any(|&(a, b)| a <= offset && offset < b)
        {
            return RangeRead::Hole {
                len: buf.len() as u64,
            };
        }
        inner.wanted = Some((volume, offset, buf.len()));
        RangeRead::NotYet
    }
}

/// The backup's volumes as a zipfast [`VolumeSource`]: every byte is answered from [`Reads`] or
/// asked for (`NotYet`), and read by [`drive`] between polls.
struct BackupVolumes {
    reads: Reads,
}

impl VolumeSource for BackupVolumes {
    fn volume(
        &mut self,
        request: &VolumeRequest,
        _cx: &mut Context<'_>,
    ) -> ProgressiveResult<VolumeAnswer> {
        let len = self.reads.lock().sizes.get(request.index as usize).copied();
        Ok(match len {
            Some(len) => VolumeAnswer::Ready(Box::new(BackupVolume {
                reads: self.reads.clone(),
                volume: request.index,
                len,
            })),
            None => VolumeAnswer::Missing,
        })
    }
}

struct BackupVolume {
    reads: Reads,
    volume: u32,
    len: u64,
}

impl RangeSource for BackupVolume {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            id: SourceId(0x7265_7374_6172_7400_0000_0000_0000_0000 | u128::from(self.volume)),
            generation: 0,
            extent: self.len,
            length: SourceLength::Final(self.len),
        }
    }

    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        _cx: &mut Context<'_>,
    ) -> ProgressiveResult<RangeRead> {
        if offset >= self.len || buf.is_empty() {
            return Ok(RangeRead::Filled { count: 0 });
        }
        let n = buf.len().min((self.len - offset) as usize);
        Ok(self.reads.read_at(self.volume, offset, &mut buf[..n]))
    }

    fn backing_files(&self) -> &[File] {
        &[]
    }
}

#[cfg(test)]
mod tests;
