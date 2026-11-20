//! Fleet intake: before the copies of a fleet are ranked, each one is opened
//! down to the video it carries (through every stored container: 7z, RAR5,
//! RAR4, zip, a raw split, or none) and a few samples of that video are read
//! from the news servers. Copies whose video differs from the fleet's are
//! dropped; the rest are the same release, whatever their packaging.
//!
//! The containers around a video differ from post to post, so the samples
//! are taken from the payload as [`ContentMapper`] maps it, never from the
//! posted bytes. A copy that cannot be mapped (compressed, encrypted without
//! a password, unreadable) stays: it is unverified, not different.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::dupefallback::NzbgetRsByteRange;
use crate::streamrepair::{
    ContentMap, ContentMapper, ContentSource, ContentSourceSet, MemberSet, ReadError, ReadResult,
    SetMember,
};

/// One data file of a copy as its nzb lists it: name, encoded bytes and the
/// message ids of its articles in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntakeFile {
    pub name: Vec<u8>,
    pub bytes: i64,
    pub ids: Vec<Vec<u8>>,
}

/// An article as yEnc decoded it: where its bytes start in the file, the
/// file's size and the bytes.
#[derive(Debug, Clone, Default)]
pub struct Fetched {
    pub offset: i64,
    pub file_size: i64,
    pub data: Vec<u8>,
}

/// Reads articles from the news servers.
pub trait ArticleFetch {
    fn fetch(&self, message_id: &[u8]) -> Option<Fetched>;
    /// The time for the intake is up, or the program stops.
    fn expired(&self) -> bool;
}

/// Bytes read from each sample of the payload.
pub const SAMPLE_BYTES: i64 = 4096;
/// Articles one copy may fetch while it is mapped and sampled, besides
/// [`FETCH_PER_VOLUME`] for each of its files.
pub const FETCH_BUDGET: i32 = 48;
/// Articles each file of a copy adds to its budget: mapping reads every
/// volume's header, and a nested archive (a RAR set stored inside another)
/// reads the inner volumes' headers too, wherever they fall in the outer ones.
pub const FETCH_PER_VOLUME: i32 = 3;
/// The most articles one copy may fetch, however many files it has.
pub const FETCH_BUDGET_MAX: i32 = 640;

/// The articles a copy of `files` data files may fetch while it is mapped and
/// sampled.
pub fn fetch_budget(files: usize) -> i32 {
    let per_volume = i32::try_from(files)
        .unwrap_or(i32::MAX)
        .saturating_mul(FETCH_PER_VOLUME);
    FETCH_BUDGET.saturating_add(per_volume).min(FETCH_BUDGET_MAX)
}

/// What intake found about one copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Its video matches the fleet's.
    Same,
    /// Its video is another one: dropped.
    Different,
    /// It could not be opened or read: kept, unverified.
    Unverified,
}

/// The payload of a copy: its size and a checksum of each sample.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Print {
    pub size: i64,
    pub samples: Vec<u32>,
}

/// The nzb parameter that keeps a copy's [`inner_param_value`]: where its
/// video was sampled and the [`Print::digest`] of what was read there. Two
/// values with the same sampling and digest mean the same video inside,
/// whatever containers hold it; TwinCheck labels an alt by it.
pub const INNER_PARAM: &[u8] = b"DupeInner";

/// `<samples>.<seed hex>.<digest>`: digests compare only between copies
/// sampled at the same offsets (the same seed and sample count).
pub fn inner_param_value(print: &Print, samples: usize, seed: u64) -> String {
    format!("{samples}.{seed:x}.{}", print.digest())
}

/// How two [`INNER_PARAM`] values compare: `Some(true)` the same video,
/// `Some(false)` another video, `None` not comparable (one is missing or they
/// were sampled at other offsets).
pub fn same_inner(a: &[u8], b: &[u8]) -> Option<bool> {
    let split = |v: &[u8]| -> Option<(Vec<u8>, Vec<u8>)> {
        let dot = v.iter().rposition(|&c| c == b'.')?;
        let (scope, digest) = (&v[..dot], &v[dot + 1..]);
        (!scope.is_empty() && !digest.is_empty()).then(|| (scope.to_vec(), digest.to_vec()))
    };
    let (sa, da) = split(a)?;
    let (sb, db) = split(b)?;
    (sa == sb).then_some(da == db)
}

impl Print {
    /// The print in 16 hex digits (FNV-1a 64 over the size and the samples).
    pub fn digest(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for &b in bytes {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        eat(&self.size.to_le_bytes());
        for s in &self.samples {
            eat(&s.to_le_bytes());
        }
        format!("{h:016x}")
    }
}

struct Shared<'f> {
    fetch: &'f dyn ArticleFetch,
    budget: Cell<i32>,
}

impl Shared<'_> {
    fn get(&self, id: &[u8]) -> Option<Fetched> {
        if self.budget.get() <= 0 || self.fetch.expired() {
            return None;
        }
        self.budget.set(self.budget.get() - 1);
        self.fetch.fetch(id)
    }
}

/// One posted file read article by article.
struct ArticleMember<'f> {
    ids: Vec<Vec<u8>>,
    shared: Rc<Shared<'f>>,
    cache: RefCell<HashMap<usize, Rc<Fetched>>>,
    /// Articles that could not be read: asked for once, so a dead article
    /// read again by each header parse does not use up the budget.
    missing: RefCell<std::collections::HashSet<usize>>,
    part_size: Cell<i64>,
    file_size: Cell<i64>,
}

impl ArticleMember<'_> {
    fn part(&self, index: usize) -> Option<Rc<Fetched>> {
        if let Some(hit) = self.cache.borrow().get(&index) {
            return Some(Rc::clone(hit));
        }
        if self.missing.borrow().contains(&index) {
            return None;
        }
        let id = self.ids.get(index)?;
        let budget_before = self.shared.budget.get();
        let got = match self.shared.get(id) {
            Some(f) if !f.data.is_empty() => Rc::new(f),
            _ => {
                // only a fetch that was made says the article is gone
                if self.shared.budget.get() < budget_before {
                    self.missing.borrow_mut().insert(index);
                }
                return None;
            }
        };
        if index == 0 || self.part_size.get() <= 0 {
            self.part_size.set(got.data.len() as i64);
        }
        if got.file_size > 0 {
            self.file_size.set(got.file_size);
        }
        self.cache.borrow_mut().insert(index, Rc::clone(&got));
        Some(got)
    }
}

impl ContentSource for ArticleMember<'_> {
    fn size(&self) -> i64 {
        if self.file_size.get() <= 0 {
            let _ = self.part(0);
        }
        self.file_size.get().max(0)
    }

    fn read(&self, offset: i64, buf: &mut [u8]) -> ReadResult {
        if self.ids.is_empty() {
            return Err(ReadError::NoSource);
        }
        if offset < 0 {
            return Err(ReadError::OutOfRange);
        }
        if self.part_size.get() <= 0 && self.part(0).is_none() {
            return Err(ReadError::Io);
        }
        let last = self.ids.len() - 1;
        let mut pos = offset;
        let mut done = 0usize;
        while done < buf.len() {
            let part_size = self.part_size.get().max(1);
            let mut index = ((pos / part_size) as usize).min(last);
            // Parts are mostly the same size; step to the one holding `pos`.
            let mut steps = 0;
            let art = loop {
                let art = self.part(index).ok_or(ReadError::Io)?;
                let end = art.offset + art.data.len() as i64;
                if pos < art.offset && index > 0 && steps < 4 {
                    index -= 1;
                } else if pos >= end && index < last && steps < 4 {
                    index += 1;
                } else if pos >= art.offset && pos < end {
                    break art;
                } else {
                    return Err(ReadError::Unavailable);
                }
                steps += 1;
            };
            let from = (pos - art.offset) as usize;
            let n = (art.data.len() - from).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&art.data[from..from + n]);
            done += n;
            pos += n as i64;
        }
        Ok(())
    }
}

struct IntakeSources<'f> {
    shared: Rc<Shared<'f>>,
    members: Vec<ArticleMember<'f>>,
}

impl ContentSourceSet for IntakeSources<'_> {
    fn get_source(&self, member_index: i32) -> Option<&dyn ContentSource> {
        usize::try_from(member_index)
            .ok()
            .and_then(|i| self.members.get(i))
            .map(|m| m as &dyn ContentSource)
    }

    fn aborted(&self) -> bool {
        self.shared.budget.get() <= 0 || self.shared.fetch.expired()
    }

    fn grant_fetches(&self, extra: i32) {
        self.shared
            .budget
            .set(self.shared.budget.get() + extra.max(0));
    }
}

fn base_name(path: &[u8]) -> &[u8] {
    path.rsplit(|&c| c == b'/' || c == b'\\')
        .next()
        .unwrap_or(path)
}

/// Where the samples are taken in a payload of `size` bytes: its start (the
/// container header of the video: an mkv's EBML header and segment info,
/// with its unique segment id) and `count` more spread at random, the same
/// for every copy of a fleet (`seed`).
pub fn sample_offsets(size: i64, count: usize, seed: u64) -> Vec<i64> {
    if size <= 0 {
        return Vec::new();
    }
    let span = (size - SAMPLE_BYTES).max(0);
    let mut out = vec![0i64];
    let mut x = seed ^ (size as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for _ in 0..count {
        // xorshift64*
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        out.push(if span == 0 {
            0
        } else {
            (r % (span as u64 + 1)) as i64
        });
    }
    out
}

/// Smallest file a RAR key may stand for: a smaller one is a sample or nfo
/// spilling into the last volume, not the video (tests use small fixtures).
#[cfg(not(test))]
const RAR_KEY_MIN: u64 = crate::twinfields::MIN_KEY_SIZE;
#[cfg(test)]
const RAR_KEY_MIN: u64 = 0;
/// Bytes of a RAR set's last volume read for its file header: within the
/// first article of any posting.
const RAR_KEY_HEAD: usize = 64 * 1024;

/// The size and CRC32 of the largest file of a copy's archive set: for RAR
/// from the file header at the start of the last volume
/// ([`crate::twinfields::rar_last_volume_key`], one article), for 7z from the
/// 7z header ([`crate::twinfields::seven_zip_key`]). A 7z with a password but
/// without `-mhe` keeps that header in the clear, so a copy [`print_copy`]
/// cannot open without the password still gets a key TwinCheck can match to
/// the fleet's video. Reads the first article of the first and the last
/// volume and the header articles near the end. None for other packings.
pub fn inner_key_copy(
    files: &[IntakeFile],
    fetch: &dyn ArticleFetch,
) -> Option<crate::twinfields::InnerKey> {
    let files: Vec<&IntakeFile> = files.iter().filter(|f| !f.ids.is_empty()).collect();
    let members: Vec<SetMember> = files
        .iter()
        .map(|f| SetMember::new(base_name(&f.name), f.bytes))
        .collect();
    let sets = ContentMapper::group_sets(&members);
    let set = largest_set(&sets, &files)?;
    let shared = Rc::new(Shared {
        fetch,
        budget: Cell::new(FETCH_BUDGET),
    });
    let vols: Vec<ArticleMember> = set
        .members
        .iter()
        .filter_map(|&i| files.get(i as usize))
        .map(|f| ArticleMember {
            ids: f.ids.clone(),
            shared: Rc::clone(&shared),
            cache: RefCell::new(HashMap::new()),
            missing: RefCell::new(std::collections::HashSet::new()),
            part_size: Cell::new(0),
            file_size: Cell::new(0),
        })
        .collect();
    let first = vols.first()?;
    let mut magic = [0u8; 6];
    first.read(0, &mut magic).ok()?;
    if magic.starts_with(b"Rar!") {
        // the last volume's first bytes hold its file header, which records
        // the whole file's size and CRC32
        let last = vols.last()?;
        let len = usize::try_from(last.size())
            .ok()
            .filter(|&n| n > 0)?
            .min(RAR_KEY_HEAD);
        let mut head = vec![0u8; len];
        last.read(0, &mut head).ok()?;
        // RAR volumes are one size but the last: a full-size "last" volume
        // means the copy lacks the set's real last volumes, and a middle
        // volume would key the file that ends there, not the video
        let first_len = u64::try_from(first.size()).unwrap_or(0);
        let last_len = u64::try_from(last.size()).unwrap_or(0);
        if vols.len() > 1 && last_len >= first_len {
            return None;
        }
        // the last volume shows the file packed last, not the largest: trust
        // it only when it fills most of the set, so it must be the largest
        // (two CDs, a featurette or a season pack give no key, not CD2's).
        // The set's size is measured from its volumes, and the nzb's byte
        // counts only ever raise it
        let measured = first_len
            .saturating_mul(vols.len() as u64 - 1)
            .saturating_add(last_len);
        // counted over the whole copy (but par2 and samples), so a 2CD
        // release posted as two sets keys neither CD
        let listed: u64 = files
            .iter()
            .filter(|f| {
                let n = base_name(&f.name).to_ascii_lowercase();
                !n.ends_with(b".par2") && !n.windows(6).any(|w| w == b"sample")
            })
            .map(|f| u64::try_from(f.bytes).unwrap_or(0))
            .fold(0u64, u64::saturating_add);
        let set_bytes = measured.max(listed);
        return crate::twinfields::rar_last_volume_key(&head, vols.len() > 1, RAR_KEY_MIN)
            .filter(|k| k.size.saturating_mul(2) > set_bytes);
    }
    if magic != crate::twinfields::SEVEN_ZIP_MAGIC {
        return None;
    }
    // 7-Zip splits into equal volumes but the last
    let first_len = u64::try_from(first.size()).ok().filter(|&n| n > 0)?;
    let last_len = u64::try_from(vols.last()?.size()).ok().filter(|&n| n > 0)?;
    let mut lens = vec![first_len; vols.len()];
    *lens.last_mut()? = last_len;
    let first_file = files.get(usize::try_from(*set.members.first()?).ok()?)?;
    let name = String::from_utf8_lossy(base_name(&first_file.name)).into_owned();
    crate::twinfields::seven_zip_key(
        &lens,
        |vol, offset, buf| {
            vols.get(vol as usize)
                .is_some_and(|m| i64::try_from(offset).is_ok_and(|o| m.read(o, buf).is_ok()))
        },
        &name,
    )
    .ok()
}

/// The set with the most bytes, sample sets left out: a multipart archive
/// outweighs a sample bigger than any one volume.
fn largest_set<'s>(sets: &'s [MemberSet], files: &[&IntakeFile]) -> Option<&'s MemberSet> {
    let is_sample = |i: i32| {
        files.get(i as usize).is_some_and(|f| {
            f.name
                .to_ascii_lowercase()
                .windows(6)
                .any(|w| w == b"sample")
        })
    };
    sets.iter()
        .filter(|s| !s.members.iter().all(|&i| is_sample(i)))
        .max_by_key(|s| {
            s.members
                .iter()
                .filter_map(|&i| files.get(i as usize).map(|f| f.bytes.max(0)))
                .sum::<i64>()
        })
}

/// Opens a copy down to its video and reads `count` random samples of it
/// (plus its start), or says why it could not.
pub fn print_copy(
    files: &[IntakeFile],
    fetch: &dyn ArticleFetch,
    password: Option<&[u8]>,
    count: usize,
    seed: u64,
) -> Result<Print, String> {
    let files: Vec<&IntakeFile> = files.iter().filter(|f| !f.ids.is_empty()).collect();
    if files.is_empty() {
        return Err("no data files".to_string());
    }
    let members: Vec<SetMember> = files
        .iter()
        .map(|f| SetMember::new(base_name(&f.name), f.bytes))
        .collect();
    let shared = Rc::new(Shared {
        fetch,
        budget: Cell::new(fetch_budget(files.len())),
    });
    let sources = IntakeSources {
        shared: Rc::clone(&shared),
        members: files
            .iter()
            .map(|f| ArticleMember {
                ids: f.ids.clone(),
                shared: Rc::clone(&shared),
                cache: RefCell::new(HashMap::new()),
                missing: RefCell::new(std::collections::HashSet::new()),
                part_size: Cell::new(0),
                file_size: Cell::new(0),
            })
            .collect(),
    };
    // The video is the set with the most bytes, samples left out.
    let sets = ContentMapper::group_sets(&members);
    let Some(set) = largest_set(&sets, &files) else {
        return Err("no file set".to_string());
    };
    let map: ContentMap = ContentMapper::build_map_with_tls(
        &members, set, &sources, password, true,
    )
    .map_err(|e| match e {
        // an aborted mapping says nothing; never log an empty reason
        crate::streamrepair::MapError::Aborted if shared.budget.get() <= 0 => {
            "archive could not be mapped: every article it may read was used up".to_string()
        }
        crate::streamrepair::MapError::Aborted if fetch.expired() => {
            "archive could not be mapped: no time left".to_string()
        }
        crate::streamrepair::MapError::Aborted => "archive could not be mapped".to_string(),
        e => e.to_string(),
    })?;
    let size = map.inner_size();
    if size <= 0 {
        return Err("empty payload".to_string());
    }
    let mut samples = Vec::with_capacity(count + 1);
    let mut buf = Vec::new();
    for offset in sample_offsets(size, count, seed) {
        let range = NzbgetRsByteRange {
            offset,
            size: SAMPLE_BYTES.min(size - offset),
        };
        if map.read_donor_inner(&sources, range, &mut buf).is_err() {
            return Err("payload unreadable".to_string());
        }
        // Only a video's own bytes may drop a copy: an archive under an
        // obfuscated name maps as a bare file, and its bytes say nothing.
        if offset == 0 && !crate::donorkind::media_magic(&buf) {
            return Err("payload does not start like a video".to_string());
        }
        samples.push(crate::dupefallback::crc32_ieee(&buf));
    }
    // Articles that tell another file size than the nzb lists (by more than
    // the yEnc overhead) are not the posted file: nothing to compare.
    for (m, f) in sources.members.iter().zip(&files) {
        let seen = m.file_size.get();
        if seen > 0
            && f.bytes > 0
            && (seen.saturating_mul(2) < f.bytes || seen > f.bytes.saturating_mul(2))
        {
            return Err(format!(
                "articles of {} tell {} bytes, the nzb {}",
                String::from_utf8_lossy(&f.name),
                seen,
                f.bytes
            ));
        }
    }
    Ok(Print { size, samples })
}

/// The fleet's video: the print most copies share; between equally many, the
/// one of the earliest copy (the client sends its pick first). None when no
/// copy has a print.
pub fn fleet_print(prints: &[Option<Print>]) -> Option<Print> {
    let mut groups: Vec<(&Print, usize, usize)> = Vec::new(); // print, count, first
    for (i, p) in prints.iter().enumerate() {
        let Some(p) = p else { continue };
        match groups.iter_mut().find(|g| g.0 == p) {
            Some(g) => g.1 += 1,
            None => groups.push((p, 1, i)),
        }
    }
    groups
        .iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)))
        .map(|g| g.0.clone())
}

/// Which copies carry `fleet`, the fleet's video. Copies without a print are
/// unverified.
pub fn judge_against(prints: &[Option<Print>], fleet: Option<&Print>) -> Vec<Verdict> {
    prints
        .iter()
        .map(|p| match (p, fleet) {
            (None, _) | (_, None) => Verdict::Unverified,
            (Some(p), Some(f)) if p == f => Verdict::Same,
            _ => Verdict::Different,
        })
        .collect()
}

/// Which copies carry the fleet's video (see [`fleet_print`]). Copies without
/// a print are unverified.
pub fn judge(prints: &[Option<Print>]) -> Vec<Verdict> {
    judge_against(prints, fleet_print(prints).as_ref())
}

/// Samples per copy, besides the payload's start, from the BODY checks of
/// the health sampling: weak (5) reads 2, medium (20) 4, hard (60) 8.
pub fn samples_for_body_checks(body_checks: i32) -> usize {
    (body_checks.max(0) / 5).clamp(2, 8) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_prints_share_a_digest() {
        let a = Print {
            size: 10,
            samples: vec![1, 2, 3],
        };
        assert_eq!(a.digest(), a.clone().digest());
        assert_eq!(a.digest().len(), 16);
        assert_ne!(
            a.digest(),
            Print {
                size: 10,
                samples: vec![1, 2, 4]
            }
            .digest()
        );
        assert_ne!(
            a.digest(),
            Print {
                size: 11,
                samples: vec![1, 2, 3]
            }
            .digest()
        );
    }

    /// Articles of fixed-size parts of named files, served from memory.
    struct Memory {
        files: HashMap<Vec<u8>, (Vec<u8>, usize)>, // id prefix -> (file bytes, part size)
        fetched: Cell<i32>,
    }

    impl ArticleFetch for Memory {
        fn fetch(&self, id: &[u8]) -> Option<Fetched> {
            self.fetched.set(self.fetched.get() + 1);
            let s = std::str::from_utf8(id).ok()?;
            let (prefix, n) = s.rsplit_once('-')?;
            let (data, part) = self.files.get(prefix.as_bytes())?;
            let n: usize = n.parse().ok()?;
            let start = n * part;
            if start >= data.len() {
                return None;
            }
            let end = (start + part).min(data.len());
            Some(Fetched {
                offset: start as i64,
                file_size: data.len() as i64,
                data: data[start..end].to_vec(),
            })
        }
        fn expired(&self) -> bool {
            false
        }
    }

    fn video(len: usize, seed: u8) -> Vec<u8> {
        let mut v = b"\x1aE\xdf\xa3".to_vec();
        let mut x = u32::from(seed).wrapping_mul(2_654_435_761).wrapping_add(7);
        while v.len() < len {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            v.push(x as u8);
        }
        v
    }

    fn copy(
        mem: &mut Memory,
        prefix: &str,
        name: &str,
        data: Vec<u8>,
        part: usize,
    ) -> Vec<IntakeFile> {
        let parts = data.len().div_ceil(part);
        let file = IntakeFile {
            name: name.as_bytes().to_vec(),
            bytes: data.len() as i64,
            ids: (0..parts)
                .map(|i| format!("{prefix}-{i}").into_bytes())
                .collect(),
        };
        mem.files.insert(prefix.as_bytes().to_vec(), (data, part));
        vec![file]
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/src/twinfields/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn a_passworded_7z_without_mhe_gets_its_key_but_no_print() {
        let mut mem = Memory {
            files: HashMap::new(),
            fetched: Cell::new(0),
        };
        let video = crate::twinfields::InnerKey {
            size: 49156,
            crc32: 0x771f_bd29,
        };
        let pass = copy(&mut mem, "p", "rel.7z", fixture("pass.7z"), 4000);
        assert!(print_copy(&pass, &mem, None, 2, 1).is_err(), "no password");
        assert_eq!(inner_key_copy(&pass, &mem), Some(video));
        let plain = copy(&mut mem, "z", "rel.7z", fixture("plain.7z"), 3000);
        assert_eq!(inner_key_copy(&plain, &mem), Some(video));
        let mut split = Vec::new();
        for i in 1..=3 {
            let name = format!("rel.7z.00{i}");
            split.extend(copy(
                &mut mem,
                &format!("s{i}"),
                &name,
                fixture(&format!("pass-split.{}", &name[4..])),
                4000,
            ));
        }
        assert_eq!(inner_key_copy(&split, &mem), Some(video));
        let mhe = copy(&mut mem, "h", "rel.7z", fixture("pass-mhe.7z"), 4000);
        assert_eq!(inner_key_copy(&mhe, &mem), None);
        let rar = copy(
            &mut mem,
            "r",
            "rel.part1.rar",
            fixture("rerar-a.part1.rar"),
            4000,
        );
        // a first volume alone records no whole-file CRC
        assert_eq!(inner_key_copy(&rar, &mem), None);
    }

    #[test]
    fn a_rar_set_gets_its_key_from_the_last_volume() {
        let mut mem = Memory {
            files: HashMap::new(),
            fetched: Cell::new(0),
        };
        let key_of = |mem: &mut Memory, tag: &str, set: &str| {
            let mut files = Vec::new();
            for i in 1..=3 {
                files.extend(copy(
                    mem,
                    &format!("{tag}{i}"),
                    &format!("rel.part{i}.rar"),
                    fixture(&format!("{set}.part{i}.rar")),
                    4000,
                ));
            }
            files
        };
        let a = key_of(&mut mem, "a", "rerar-a");
        let c = key_of(&mut mem, "c", "rerar-c");
        let video = crate::twinfields::InnerKey {
            size: 49156,
            crc32: 0x771f_bd29,
        };
        assert_eq!(inner_key_copy(&a, &mem), Some(video));
        // a re-RAR of the same file records the same size and CRC32
        assert_eq!(inner_key_copy(&c, &mem), Some(video));
        // when the file seen is not most of the set (a second CD packed
        // before it), it may not be the largest: no key
        let mut two_cds = a.clone();
        two_cds[0].bytes += 60_000;
        assert_eq!(inner_key_copy(&two_cds, &mem), None);
        // with no byte counts in the nzb the set is measured from its volumes
        let mut no_bytes = a.clone();
        no_bytes.iter_mut().for_each(|f| f.bytes = 0);
        assert_eq!(inner_key_copy(&no_bytes, &mem), Some(video));
        // a copy lacking its real last volume keys nothing from a full-size
        // "last" one: here the final volume padded to full size, in place of
        // part 2, would otherwise give the video's key
        let mut padded = fixture("rerar-a.part3.rar");
        padded.resize(20480, 0);
        let mut short = copy(
            &mut mem,
            "s1",
            "rel.part1.rar",
            fixture("rerar-a.part1.rar"),
            4000,
        );
        short.extend(copy(&mut mem, "s2", "rel.part2.rar", padded, 4000));
        assert_eq!(inner_key_copy(&short, &mem), None);
        // a second set as large as the first (CD2 posted on its own) leaves
        // the video under half the copy: no key
        let mut two_sets = a.clone();
        two_sets.extend(copy(
            &mut mem,
            "d",
            "rel-cd2.rar",
            fixture("rerar-c.part1.rar"),
            4000,
        ));
        two_sets.last_mut().unwrap().bytes = 49_000;
        assert_eq!(inner_key_copy(&two_sets, &mem), None);
    }

    #[test]
    fn bare_copies_of_one_video_match_and_another_does_not() {
        let mut mem = Memory {
            files: HashMap::new(),
            fetched: Cell::new(0),
        };
        let a = copy(&mut mem, "a", "Show.mkv", video(200_000, 1), 7_000);
        let b = copy(&mut mem, "b", "show-grp.mkv", video(200_000, 1), 9_000);
        let c = copy(&mut mem, "c", "Show.mkv", video(200_000, 2), 7_000);
        let prints: Vec<Option<Print>> = [&a, &b, &c]
            .iter()
            .map(|f| print_copy(f, &mem, None, 4, 42).ok())
            .collect();
        assert!(prints.iter().all(Option::is_some));
        assert_eq!(
            prints[0], prints[1],
            "same video, other part sizes and names"
        );
        assert_ne!(prints[0], prints[2]);
        assert_eq!(
            judge(&prints),
            vec![Verdict::Same, Verdict::Same, Verdict::Different]
        );
    }

    #[test]
    fn a_split_movie_wins_over_a_sample_bigger_than_any_volume() {
        let mut mem = Memory {
            files: HashMap::new(),
            fetched: Cell::new(0),
        };
        let movie = video(300_000, 5);
        let bare = copy(&mut mem, "m", "movie.mkv", movie.clone(), 8_000);
        // the same movie as a raw split of three 100 KB volumes, plus a
        // 150 KB sample of another video
        let mut split = Vec::new();
        for (n, chunk) in movie.chunks(100_000).enumerate() {
            split.extend(copy(
                &mut mem,
                &format!("s{n}"),
                &format!("movie.mkv.{:03}", n + 1),
                chunk.to_vec(),
                8_000,
            ));
        }
        split.extend(copy(
            &mut mem,
            "x",
            "movie-sample.mkv",
            video(150_000, 6),
            8_000,
        ));
        let a = print_copy(&bare, &mem, None, 4, 9).expect("bare");
        let b = print_copy(&split, &mem, None, 4, 9).expect("split");
        assert_eq!(a, b, "the split movie is read, not the sample");
    }

    #[test]
    fn an_archive_under_an_obfuscated_name_is_unverified() {
        let mut mem = Memory {
            files: HashMap::new(),
            fetched: Cell::new(0),
        };
        let zip = stored_zip("movie.mkv", &video(100_000, 7));
        let obf = copy(&mut mem, "o", "a8f3c2d19e", zip, 8_000);
        let got = print_copy(&obf, &mem, None, 4, 3);
        assert!(got.is_err(), "{got:?}");
        assert!(crate::donorkind::media_magic(&video(64, 1)));
        assert!(!crate::donorkind::media_magic(b"PK\x03\x04 not a video"));
    }

    /// A zip archive holding `name` stored (not compressed).
    fn stored_zip(name: &str, data: &[u8]) -> Vec<u8> {
        let crc = crate::dupefallback::crc32_ieee(data);
        let n = name.as_bytes();
        let mut z = Vec::new();
        let local = |z: &mut Vec<u8>| {
            z.extend_from_slice(&[0x50, 0x4b, 3, 4, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            z.extend_from_slice(&crc.to_le_bytes());
            z.extend_from_slice(&(data.len() as u32).to_le_bytes());
            z.extend_from_slice(&(data.len() as u32).to_le_bytes());
            z.extend_from_slice(&(n.len() as u16).to_le_bytes());
            z.extend_from_slice(&[0, 0]);
            z.extend_from_slice(n);
        };
        local(&mut z);
        z.extend_from_slice(data);
        let cd = z.len() as u32;
        z.extend_from_slice(&[0x50, 0x4b, 1, 2, 20, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(n.len() as u16).to_le_bytes());
        z.extend_from_slice(&[0u8; 12]);
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(n);
        let cd_size = z.len() as u32 - cd;
        z.extend_from_slice(&[0x50, 0x4b, 5, 6, 0, 0, 0, 0, 1, 0, 1, 0]);
        z.extend_from_slice(&cd_size.to_le_bytes());
        z.extend_from_slice(&cd.to_le_bytes());
        z.extend_from_slice(&[0, 0]);
        z
    }

    #[test]
    fn the_budget_grows_with_the_volumes_up_to_a_cap() {
        assert_eq!(fetch_budget(0), FETCH_BUDGET);
        assert_eq!(fetch_budget(1), FETCH_BUDGET + FETCH_PER_VOLUME);
        // a nested set of 60 outer volumes can read every outer and inner header
        assert!(fetch_budget(60) >= 2 * 60 + FETCH_BUDGET);
        assert_eq!(fetch_budget(10_000), FETCH_BUDGET_MAX);
        assert_eq!(fetch_budget(usize::MAX), FETCH_BUDGET_MAX);
    }

    #[test]
    fn a_dead_article_is_fetched_once() {
        let mem = Memory {
            files: HashMap::new(),
            fetched: Cell::new(0),
        };
        let shared = Rc::new(Shared {
            fetch: &mem,
            budget: Cell::new(FETCH_BUDGET),
        });
        let member = ArticleMember {
            ids: vec![b"gone-0".to_vec(), b"gone-1".to_vec()],
            shared: Rc::clone(&shared),
            cache: RefCell::new(HashMap::new()),
            missing: RefCell::new(std::collections::HashSet::new()),
            part_size: Cell::new(0),
            file_size: Cell::new(0),
        };
        // each header parse reads the volume again; a dead article costs one fetch
        for _ in 0..10 {
            let mut buf = [0u8; 16];
            assert!(member.read(0, &mut buf).is_err());
        }
        assert_eq!(mem.fetched.get(), 1);
        assert_eq!(shared.budget.get(), FETCH_BUDGET - 1);
    }

    #[test]
    fn copies_in_stored_containers_match_through_every_layer() {
        let mut mem = Memory {
            files: HashMap::new(),
            fetched: Cell::new(0),
        };
        let v = video(300_000, 5);
        let bare = copy(&mut mem, "bare", "Show.mkv", v.clone(), 8_000);
        let zipped = copy(
            &mut mem,
            "zip",
            "Show.zip",
            stored_zip("Show.mkv", &v),
            9_000,
        );
        let inner = stored_zip("Show.mkv", &v);
        let nested = copy(
            &mut mem,
            "nest",
            "Show-x.zip",
            stored_zip("inner.zip", &inner),
            7_500,
        );
        let other = copy(
            &mut mem,
            "oth",
            "Show.zip",
            stored_zip("Show.mkv", &video(300_000, 6)),
            9_000,
        );
        let prints: Vec<Result<Print, String>> = [&bare, &zipped, &nested, &other]
            .iter()
            .map(|f| print_copy(f, &mem, None, 4, 7))
            .collect();
        for p in &prints {
            assert!(p.is_ok(), "{p:?}");
        }
        let prints: Vec<Option<Print>> = prints.into_iter().map(Result::ok).collect();
        assert_eq!(
            judge(&prints),
            vec![
                Verdict::Same,
                Verdict::Same,
                Verdict::Same,
                Verdict::Different
            ]
        );
    }

    #[test]
    fn judge_prefers_the_majority_then_the_first_copy() {
        let p = |s: i64| {
            Some(Print {
                size: s,
                samples: vec![1],
            })
        };
        assert_eq!(
            judge(&[p(1), p(2), p(2), None]),
            vec![
                Verdict::Different,
                Verdict::Same,
                Verdict::Same,
                Verdict::Unverified
            ]
        );
        assert_eq!(
            judge(&[p(1), p(2)]),
            vec![Verdict::Same, Verdict::Different]
        );
        assert_eq!(judge(&[None, None]), vec![Verdict::Unverified; 2]);
        // a later check compares with the fleet's video found at submission
        let a = Print {
            size: 10,
            samples: vec![1],
        };
        let b = Print {
            size: 10,
            samples: vec![2],
        };
        assert_eq!(
            fleet_print(&[None, Some(b.clone()), Some(a.clone()), Some(a.clone())]),
            Some(a.clone())
        );
        assert_eq!(
            judge_against(&[Some(b.clone()), Some(a.clone()), None], Some(&a)),
            vec![Verdict::Different, Verdict::Same, Verdict::Unverified]
        );
        assert_eq!(
            judge_against(&[Some(a.clone())], None),
            vec![Verdict::Unverified]
        );
    }

    #[test]
    fn samples_start_at_the_header_and_repeat_per_seed() {
        let a = sample_offsets(1_000_000, 4, 9);
        assert_eq!(a.len(), 5);
        assert_eq!(a[0], 0);
        assert_eq!(a, sample_offsets(1_000_000, 4, 9));
        assert!(a.iter().all(|&o| o + SAMPLE_BYTES <= 1_000_000));
        assert_eq!(sample_offsets(100, 2, 1), vec![0, 0, 0]);
    }

    #[test]
    fn a_copy_that_cannot_be_read_is_unverified() {
        let mem = Memory {
            files: HashMap::new(),
            fetched: Cell::new(0),
        };
        let files = vec![IntakeFile {
            name: b"x.mkv".to_vec(),
            bytes: 1000,
            ids: vec![b"gone-0".to_vec()],
        }];
        assert!(print_copy(&files, &mem, None, 2, 1).is_err());
    }
}
