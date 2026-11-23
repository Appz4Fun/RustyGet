//! Twin and alt evidence that needs no par2 set.
//!
//! TwinCheck proves a twin from par2 file lists when both copies carry one.
//! Without par2 it reads articles of both copies at the same places and
//! compares them. This module plans and judges those reads, and adds three
//! more kinds of evidence:
//!
//! - **Header-bearing articles.** An archive's headers sit in the first and
//!   the last article of each volume. Re-packing the same files (a new
//!   modification time, the same rar version and switches) changes exactly
//!   those articles and leaves every article in between byte-identical
//!   (measured with rar 7.00: 8 volumes, only the first and last article of
//!   each differ). So a sample must include header articles to tell a twin
//!   from such a re-pack, which this module calls a *re-RAR*. A re-RAR is an
//!   alt under the project's definitions, but one whose middle articles are
//!   the same bytes as the primary's and may be lent one for one.
//! - **Per-creation random fields.** Encrypted archives carry random values
//!   that every new archiver run draws afresh: the RAR5 `-hp` header salt
//!   (the same in every volume of one set), the RAR5 file salt and IV, the
//!   RAR4 salts. Two copies with the same value come from the same run; two
//!   with different values do not. zipfast reads them from the first article.
//! - **Other article sizes.** Two posts of the same files with other article
//!   sizes (716800 and 768000 bytes) share no article boundaries except on
//!   the grid of the sizes' least common multiple (par-rs
//!   [`par_rs::lcm_grid`]). Articles that start on that grid start at the
//!   same file offset in both posts and are compared there.
//!
//! And for alts: a 7z archive with a password but without `-mhe` keeps its
//! header in the clear, with each file's size and CRC32. [`seven_zip_key`]
//! reads that header through zipfast without the password, so a copy that
//! cannot be opened can still be matched to the fleet's video by its
//! [`InnerKey`].
//!
//! No single value links copies on its own: a size or a CRC32 alone never
//! makes a twin or an alt.

use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use zipfast::map::{CipherParams, MapBuilder, MapFormat, MapOptions, MemberMap};
use zipfast::progressive::{
    ProgressiveResult, RangeRead, RangeSource, SourceId, SourceInfo, SourceLength, VolumeAnswer,
    VolumeRequest, VolumeSource,
};

/// Middle articles sampled besides the header articles.
pub const MIDDLE_SAMPLES: usize = 6;
/// Files whose header articles (first and last) are sampled: the first, the
/// middle and the last data file.
pub const HEADER_FILES: usize = 3;

/// One pair of articles to read and compare: the same file in both copies,
/// article `seg_a` of copy A and `seg_b` of copy B.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplePair {
    pub file: usize,
    pub seg_a: usize,
    pub seg_b: usize,
    /// The first or last article of the file: where archive headers are.
    pub header: bool,
    /// The last article of the file in both copies: they end together, so
    /// only the bytes both hold (the file's tail) are compared.
    pub tail: bool,
}

/// The files whose header articles are sampled.
fn header_files(n_files: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(HEADER_FILES);
    for f in [0, n_files / 2, n_files.saturating_sub(1)] {
        if f < n_files && !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// `count` places spread evenly over `places` (none repeated).
fn spread<T: Copy + PartialEq>(places: &[T], count: usize) -> Vec<T> {
    let mut out = Vec::new();
    if places.is_empty() || count == 0 {
        return out;
    }
    for n in 0..count {
        let p = places[((2 * n + 1) * places.len()) / (2 * count)];
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Sample pairs for two copies posted with the same articles: the same
/// number of data files, and every article the same encoded size. The first
/// and last article of [`HEADER_FILES`] files, then `middle` articles spread
/// over the articles between them. None when the layouts differ.
pub fn plan_same_layout(
    a_counts: &[usize],
    a_bytes: &[i64],
    b_counts: &[usize],
    b_bytes: &[i64],
    middle: usize,
) -> Option<Vec<SamplePair>> {
    if a_counts.is_empty() || a_counts != b_counts || a_bytes != b_bytes {
        return None;
    }
    if a_counts.iter().sum::<usize>() != a_bytes.len() {
        return None;
    }
    let mut plan = Vec::new();
    for f in header_files(a_counts.len()) {
        let n = a_counts[f];
        if n == 0 {
            continue;
        }
        plan.push(SamplePair {
            file: f,
            seg_a: 0,
            seg_b: 0,
            header: true,
            tail: false,
        });
        if n > 1 {
            plan.push(SamplePair {
                file: f,
                seg_a: n - 1,
                seg_b: n - 1,
                header: true,
                tail: false,
            });
        }
    }
    let mut inner = Vec::new();
    for (f, &n) in a_counts.iter().enumerate() {
        for k in 1..middle_end(n) {
            inner.push((f, k));
        }
    }
    for (f, k) in spread(&inner, middle) {
        plan.push(SamplePair {
            file: f,
            seg_a: k,
            seg_b: k,
            header: false,
            tail: false,
        });
    }
    (!plan.is_empty()).then_some(plan)
}

/// Sample pairs for two copies of the same files posted with other article
/// sizes: `part_a` and `part_b` decoded bytes per article (all but each
/// file's last). Middle articles are taken where both posts start an article
/// at the same file offset, the multiples of the sizes' least common
/// multiple, away from each file's first and last article. None when the
/// file counts differ, the sizes are equal (use [`plan_same_layout`]) or a
/// size is zero.
pub fn plan_lcm(
    a_counts: &[usize],
    b_counts: &[usize],
    part_a: u64,
    part_b: u64,
    middle: usize,
) -> Option<Vec<SamplePair>> {
    if a_counts.is_empty() || a_counts.len() != b_counts.len() || part_a == part_b {
        return None;
    }
    let grid = par_rs::lcm_grid(&[part_a, part_b])?;
    let mut plan = Vec::new();
    for f in header_files(a_counts.len()) {
        let (na, nb) = (a_counts[f], b_counts[f]);
        if na == 0 || nb == 0 {
            continue;
        }
        plan.push(SamplePair {
            file: f,
            seg_a: 0,
            seg_b: 0,
            header: true,
            tail: false,
        });
        if na > 1 || nb > 1 {
            plan.push(SamplePair {
                file: f,
                seg_a: na - 1,
                seg_b: nb - 1,
                header: true,
                tail: true,
            });
        }
    }
    let mut inner = Vec::new();
    for (f, (&na, &nb)) in a_counts.iter().zip(b_counts).enumerate() {
        let mut k: u64 = 1;
        loop {
            let Some(offset) = grid.checked_mul(k) else {
                break;
            };
            let (sa, sb) = (offset / part_a, offset / part_b);
            // strictly inside: never a file's first or last article, nor
            // its last eighth (see [`middle_end`])
            if sa >= middle_end(na) as u64 || sb >= middle_end(nb) as u64 {
                break;
            }
            inner.push((f, sa as usize, sb as usize));
            k += 1;
        }
    }
    for (f, sa, sb) in spread(&inner, middle) {
        plan.push(SamplePair {
            file: f,
            seg_a: sa,
            seg_b: sb,
            header: false,
            tail: false,
        });
    }
    (!plan.is_empty()).then_some(plan)
}

/// Whether two decoded articles agree on the file bytes both hold: `a` at
/// file offset `a_off`, `b` at `b_off`. None when they hold no byte in common.
pub fn overlap_equal(a_off: i64, a: &[u8], b_off: i64, b: &[u8]) -> Option<bool> {
    if a_off < 0 || b_off < 0 {
        return None;
    }
    let start = a_off.max(b_off);
    let end = (a_off + a.len() as i64).min(b_off + b.len() as i64);
    if start >= end {
        return None;
    }
    let ra = (start - a_off) as usize..(end - a_off) as usize;
    let rb = (start - b_off) as usize..(end - b_off) as usize;
    Some(a[ra] == b[rb])
}

/// Whether one compared pair differs. Both articles must report the same
/// file size; outside the tail they must start at the same offset; and the
/// bytes both hold must be equal (they must hold some).
pub fn pair_differs(pair: &SamplePair, a: (i64, i64, &[u8]), b: (i64, i64, &[u8])) -> bool {
    let (a_off, a_size, a_data) = a;
    let (b_off, b_size, b_data) = b;
    if a_size > 0 && b_size > 0 && a_size != b_size {
        return true;
    }
    if !pair.tail && a_off != b_off {
        return true;
    }
    overlap_equal(a_off, a_data, b_off, b_data) != Some(true)
}

/// What the sampled articles say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleVerdict {
    /// Not enough articles could be read: ask again later.
    Retry,
    /// Every article compared is identical, header articles included.
    Twin,
    /// The middle articles are identical, some header articles differ: the
    /// same files packed again (another modification time). An alt whose
    /// middle articles may be lent one for one. `tail` is how many articles
    /// at the end of a volume differ: the last one, and with a recovery
    /// record (`-rr`) the ones before it too (measured with rar 7.00: `-rr5%`
    /// on 5000 KiB volumes makes the last two differ).
    Rerar { tail: u32 },
    /// Every article compared holds the same bytes, but the posts cut the
    /// files into articles of other sizes: the same files, never a twin
    /// (the article counts differ), so nothing is lent one for one.
    SameFiles,
    /// Middle articles differ, or a per-creation field does: another
    /// container (the video inside is checked elsewhere).
    Alt,
}

/// The `DupeSampled` value of a sampled twin (see [`SampleVerdict::param`]).
pub const TWIN_PARAM: &[u8] = b"twin:hdr";

/// The bare `DupeSampled` twin value of the older middle-only sampler: not
/// trusted, sampled again.
pub const LEGACY_TWIN_PARAM: &[u8] = b"twin";

impl SampleVerdict {
    /// The `DupeSampled` value: `twin:hdr`, `rerar:<tail>`, `samefiles`, `alt`.
    /// A twin is `twin:hdr`, not the bare `twin` the older middle-only
    /// sampler wrote: that one never compared header articles (a re-RAR
    /// passed it), so it is read as no verdict and sampled again.
    pub fn param(self) -> Vec<u8> {
        match self {
            Self::Retry => Vec::new(),
            Self::Twin => TWIN_PARAM.to_vec(),
            Self::Rerar { tail } => format!("rerar:{tail}").into_bytes(),
            Self::SameFiles => b"samefiles".to_vec(),
            Self::Alt => b"alt".to_vec(),
        }
    }

    /// Reads a `DupeSampled` value (see [`Self::param`]).
    pub fn from_param(value: &[u8]) -> Option<Self> {
        match value {
            TWIN_PARAM => Some(Self::Twin),
            b"samefiles" => Some(Self::SameFiles),
            b"alt" => Some(Self::Alt),
            _ => {
                let tail = value.strip_prefix(b"rerar:")?;
                let tail: u32 = std::str::from_utf8(tail).ok()?.parse().ok()?;
                (tail >= 1).then_some(Self::Rerar { tail })
            }
        }
    }

    /// An alt the samples themselves prove: the same files inside.
    pub fn proves_same_files(self) -> bool {
        matches!(self, Self::Rerar { .. } | Self::SameFiles)
    }
}

/// Counts of the pairs compared so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub header_compared: usize,
    pub header_differ: usize,
    pub middle_planned: usize,
    pub middle_compared: usize,
    pub middle_differ: usize,
    /// Both first articles carry a per-creation field: `Some(true)` the same,
    /// `Some(false)` different.
    pub same_creation: Option<bool>,
}

impl Tally {
    /// Counts one compared pair.
    pub fn add(&mut self, header: bool, differs: bool) {
        if header {
            self.header_compared += 1;
            self.header_differ += usize::from(differs);
        } else {
            self.middle_compared += 1;
            self.middle_differ += usize::from(differs);
        }
    }

    /// Whether the verdict is already known, so later pairs need no reading.
    pub fn settled(&self) -> bool {
        self.same_creation == Some(false) || self.middle_differ > 0
    }
}

/// The verdict of a sample. A twin needs at least one header article and
/// half the middle articles planned read and all of them identical, and the
/// same article layout (`same_layout`): posts cut into other article sizes
/// are at most [`SampleVerdict::SameFiles`]. A re-RAR's `tail` starts at 1;
/// the caller measures it.
pub fn judge(t: &Tally, stopped: bool, same_layout: bool) -> SampleVerdict {
    match judge_bytes(t, stopped) {
        SampleVerdict::Twin | SampleVerdict::Rerar { .. } if !same_layout => {
            SampleVerdict::SameFiles
        }
        v => v,
    }
}

fn judge_bytes(t: &Tally, stopped: bool) -> SampleVerdict {
    if t.same_creation == Some(false) || t.middle_differ > 0 {
        return SampleVerdict::Alt;
    }
    if stopped || t.header_compared == 0 {
        return SampleVerdict::Retry;
    }
    if t.middle_compared < t.middle_planned.div_ceil(2) {
        return SampleVerdict::Retry;
    }
    if t.header_differ > 0 {
        // with no middle article to compare, nothing shows the files inside
        // are the same
        return if t.middle_compared > 0 {
            SampleVerdict::Rerar { tail: 1 }
        } else {
            SampleVerdict::Alt
        };
    }
    SampleVerdict::Twin
}

/// The relation between a downloading copy and a donor that TwinCheck
/// sampled: the donor's `DupeSampled` when it was sampled against the
/// downloading copy, else the downloading copy's when it was sampled against
/// the donor.
pub fn sampled_relation(
    own_id: i32,
    own_sampled: &[u8],
    own_of: &[u8],
    donor_id: i32,
    donor_sampled: &[u8],
    donor_of: &[u8],
) -> Option<SampleVerdict> {
    let of = |v: &[u8], id: i32| {
        std::str::from_utf8(v)
            .ok()?
            .trim()
            .parse::<i32>()
            .ok()
            .filter(|&x| x == id)
    };
    if of(donor_of, own_id).is_some() {
        return SampleVerdict::from_param(donor_sampled);
    }
    if of(own_of, donor_id).is_some() {
        return SampleVerdict::from_param(own_sampled);
    }
    None
}

/// Whether article `part_number` (1-based) of a file of `total` articles is
/// the first or one of the last `tail`: the articles holding archive headers
/// (and a recovery record).
pub fn is_header_part(part_number: i32, total: i32, tail: u32) -> bool {
    let tail = i64::from(tail.max(1));
    part_number <= 1 || (total > 0 && i64::from(part_number) > i64::from(total) - tail)
}

/// The end (exclusive) of the articles a middle sample may take in a file of
/// `n` articles: never the last, and in a file of 8 or more not its last
/// eighth either, where a re-RAR's recovery record sits; it differs between
/// runs like a header and would read a re-RAR as an alt.
pub fn middle_end(n: usize) -> usize {
    if n >= 8 {
        n - 1 - n / 8
    } else {
        n.saturating_sub(1)
    }
}

/// The first and last volume (indices into `names`) when the names are one
/// RAR volume set and nothing else: `x.part01.rar` ... `x.partNN.rar`, or
/// `x.rar`, `x.r00` ... `x.rNN`, numbered without gaps. A re-RAR is only
/// trusted on such a list: another file beside the volumes (an nfo, a
/// sample, a second set) was not checked by the volume rules.
pub fn rar_set_ends(names: &[&str]) -> Option<(usize, usize)> {
    let lower: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    let digits = |t: &str| -> Option<u32> {
        (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
            .then(|| t.parse().ok())
            .flatten()
    };
    // (stem, volume number from 0) of each name
    let new_style = |n: &str| -> Option<(String, u32)> {
        let body = n.strip_suffix(".rar")?;
        let at = body.rfind(".part")?;
        let no = digits(&body[at + 5..])?;
        Some((body[..at].to_owned(), no.checked_sub(1)?))
    };
    let old_style = |n: &str| -> Option<(String, u32)> {
        if let Some(stem) = n.strip_suffix(".rar") {
            return Some((stem.to_owned(), 0));
        }
        let at = n.rfind(".r")?;
        let no = digits(&n[at + 2..]).filter(|_| n.len() - at == 4)?;
        Some((n[..at].to_owned(), no + 1))
    };
    let pick = |parse: &dyn Fn(&str) -> Option<(String, u32)>| -> Option<(usize, usize)> {
        let parsed: Vec<(String, u32)> = lower.iter().map(|n| parse(n)).collect::<Option<_>>()?;
        let stem = &parsed.first()?.0;
        let mut seen = vec![None; parsed.len()];
        for (i, (s, no)) in parsed.iter().enumerate() {
            let slot = seen.get_mut(usize::try_from(*no).ok()?)?;
            if s != stem || slot.is_some() {
                return None;
            }
            *slot = Some(i);
        }
        Some((seen[0]?, (*seen.last()?)?))
    };
    pick(&new_style).or_else(|| pick(&old_style))
}

/// Where the first member's data ends in a re-RAR's last volume, read from
/// the first bytes of its first volume (`first`) and of its last volume
/// (`last`; the same bytes when `single`, a one-volume archive). A re-RAR
/// rewrites every member header, so a header in the middle of a volume
/// differs too: the middle articles lend like a twin's only while one member
/// runs through every volume, from the first volume's start into the last.
/// Bytes after this offset (other members, a recovery record, the end
/// header) must count as differing. None when that is not shown.
pub fn rerar_member_end(first: &[u8], last: &[u8], single: bool) -> Option<u64> {
    let map = |head: &[u8]| {
        if !head.starts_with(b"Rar!") {
            return None;
        }
        MemberMap::from_volume(head, MapOptions::new("head.rar")).ok()
    };
    let f = map(first)?;
    let m = f.members.first()?;
    if !f.first_volume || m.split_before || m.is_dir {
        return None;
    }
    if single {
        return (!m.split_after).then_some(m.packed.end);
    }
    let l = map(last)?;
    let n = l.members.first()?;
    (m.split_after && n.split_before && !n.split_after && n.raw_name == m.raw_name)
        .then_some(n.packed.end)
}

/// Most article pairs read back from a volume's end to measure a re-RAR's
/// tail; a tail longer than this is not trusted as a re-RAR.
pub const MAX_TAIL_PROBE: usize = 24;

/// What a sampled relation allows a donor to lend for one article.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lend {
    /// The relation says nothing: the ordinary donor rules apply.
    Default,
    /// Lends like a twin, even where only twins may lend.
    LikeTwin,
    /// Must not lend this article (a re-RAR's header article).
    Never,
}

/// The lending rule for a donor in `relation` with the downloading copy, for
/// article `part_number` of a file of `total` articles. A sampled twin lends
/// like a twin; a re-RAR lends its middle articles like a twin and never a
/// header article (its first, or one of its measured tail), whose bytes
/// differ.
pub fn lend_rule(relation: Option<SampleVerdict>, part_number: i32, total: i32) -> Lend {
    match relation {
        Some(SampleVerdict::Twin) => Lend::LikeTwin,
        Some(SampleVerdict::Rerar { tail }) if is_header_part(part_number, total, tail) => {
            Lend::Never
        }
        Some(SampleVerdict::Rerar { .. }) => Lend::LikeTwin,
        _ => Lend::Default,
    }
}

/// A per-creation random value of an archive, read from its first bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creation {
    /// What the value is, for the log.
    pub what: &'static str,
    pub bytes: Vec<u8>,
}

/// The per-creation random value in the first bytes of a RAR volume: the
/// header salt of `-hp` (RAR5 16 bytes, RAR4 8), else the first member's
/// encryption salt (RAR5 with its IV, RAR4 salt). None for unencrypted or
/// unreadable bytes and for other formats. zipfast parses the headers.
pub fn creation_of(head: &[u8]) -> Option<Creation> {
    if !head.starts_with(b"Rar!") {
        return None;
    }
    let map = MemberMap::from_volume(head, MapOptions::new("head.rar")).ok()?;
    let from = |c: &CipherParams, header: bool| -> Option<Creation> {
        match c {
            CipherParams::Rar5 { salt, iv, .. } => {
                let mut bytes = salt.to_vec();
                match (header, iv) {
                    (true, _) => Some(Creation {
                        what: "RAR5 header salt",
                        bytes,
                    }),
                    (false, Some(iv)) => {
                        bytes.extend_from_slice(iv);
                        Some(Creation {
                            what: "RAR5 file salt and IV",
                            bytes,
                        })
                    }
                    (false, None) => Some(Creation {
                        what: "RAR5 file salt",
                        bytes,
                    }),
                }
            }
            CipherParams::Rar4 { salt }
            | CipherParams::Rar4Legacy {
                salt: Some(salt), ..
            } => Some(Creation {
                what: if header {
                    "RAR4 header salt"
                } else {
                    "RAR4 file salt"
                },
                bytes: salt.to_vec(),
            }),
            _ => None,
        }
    };
    if let Some(c) = map.header_cipher.as_ref().and_then(|c| from(c, true)) {
        return Some(c);
    }
    map.members
        .iter()
        .find_map(|m| m.cipher.as_ref().and_then(|c| from(c, false)))
}

/// Whether two copies come from the same archiver run by their per-creation
/// values: None unless both have one.
pub fn same_creation(a: Option<&Creation>, b: Option<&Creation>) -> Option<bool> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a == b),
        _ => None,
    }
}

/// The nzb parameter holding a copy's [`InnerKey`]: the size and CRC32 of the
/// video inside, as its container records them.
pub const INNER_KEY_PARAM: &[u8] = b"DupeInnerCrc";
/// The smallest video whose (size, CRC32) links copies: smaller files (and
/// their sizes) repeat too often.
pub const MIN_KEY_SIZE: u64 = 50 << 20;

/// The size and CRC32 of the largest file in an archive, as its headers
/// record them (unkeyed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InnerKey {
    pub size: u64,
    pub crc32: u32,
}

impl InnerKey {
    /// `<size>:<crc32 in 8 hex digits>`.
    pub fn param(&self) -> String {
        format!("{}:{:08x}", self.size, self.crc32)
    }

    pub fn parse(value: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(value).ok()?;
        let (size, crc) = s.trim().split_once(':')?;
        if crc.len() != 8 {
            return None;
        }
        Some(Self {
            size: size.parse().ok()?,
            crc32: u32::from_str_radix(crc, 16).ok()?,
        })
    }
}

/// How two [`INNER_KEY_PARAM`] values compare: `Some(false)` another file
/// (the sizes or the CRCs differ), `Some(true)` the same file (both equal and
/// the file at least `min_size` bytes), None when a value is missing or the
/// file is too small to tell.
///
/// Sizes that differ by no more than [`HEADER_SIZED_DIFF`] are a likely alt,
/// not another file (the user's rule, 2026-11-23: a copy of the same mkv
/// without its 11,245-byte Tags tail): None, so the copy stays an unverified
/// container that borrows with per-range checks, never `other`.
pub fn same_inner_key(a: &[u8], b: &[u8], min_size: u64) -> Option<bool> {
    let (a, b) = (InnerKey::parse(a)?, InnerKey::parse(b)?);
    if a.size != b.size {
        if a.size.abs_diff(b.size) <= HEADER_SIZED_DIFF {
            return None;
        }
        return Some(false);
    }
    if a.crc32 != b.crc32 {
        return Some(false);
    }
    (a.size >= min_size).then_some(true)
}

/// The largest size difference between two videos that can still be the same
/// movie with some header or tag bytes more or less (an mkv's Tags or Cues).
pub const HEADER_SIZED_DIFF: u64 = 1024 * 1024;

/// The 7z signature.
pub const SEVEN_ZIP_MAGIC: &[u8] = b"7z\xBC\xAF\x27\x1C";

struct Wanted {
    lens: Vec<u64>,
    want: Option<(u32, u64, usize)>,
    have: (u32, u64, Vec<u8>),
}

struct Bridge {
    volume: u32,
    shared: Arc<Mutex<Wanted>>,
}

impl RangeSource for Bridge {
    fn info(&self) -> SourceInfo {
        let len = self.shared.lock().map_or(0, |s| {
            s.lens.get(self.volume as usize).copied().unwrap_or(0)
        });
        SourceInfo {
            id: SourceId(u128::from(self.volume)),
            generation: 0,
            extent: len,
            length: SourceLength::Final(len),
        }
    }

    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        _: &mut Context<'_>,
    ) -> ProgressiveResult<RangeRead> {
        let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let (vol, at, data) = &s.have;
        if *vol == self.volume {
            if let Some(rel) = offset
                .checked_sub(*at)
                .and_then(|r| usize::try_from(r).ok())
            {
                if rel < data.len() {
                    let count = buf.len().min(data.len() - rel);
                    buf[..count].copy_from_slice(&data[rel..rel + count]);
                    return Ok(RangeRead::Filled { count });
                }
            }
        }
        s.want = Some((self.volume, offset, buf.len()));
        Ok(RangeRead::NotYet)
    }

    fn backing_files(&self) -> &[std::fs::File] {
        &[]
    }
}

struct Volumes(Arc<Mutex<Wanted>>);

impl VolumeSource for Volumes {
    fn volume(
        &mut self,
        request: &VolumeRequest,
        _: &mut Context<'_>,
    ) -> ProgressiveResult<VolumeAnswer> {
        let n = self.0.lock().map_or(0, |s| s.lens.len());
        Ok(if (request.index as usize) < n {
            VolumeAnswer::Ready(Box::new(Bridge {
                volume: request.index,
                shared: Arc::clone(&self.0),
            }))
        } else {
            VolumeAnswer::Missing
        })
    }
}

/// The [`InnerKey`] a RAR set's last volume records: `head` is the start of
/// that volume (its first article is enough). Each file's last part header
/// stores the unpacked size and the CRC32 of the whole file, whatever the
/// compression, so a stored, compressed or re-RARed copy of one video gives
/// the same key. The largest file whose last part starts in `head` is taken;
/// in a set of several volumes (`multi_volume`) only a file continued from
/// the volume before counts, so a small file packed after the video (an nfo,
/// subtitles) that alone sits in the last volume never stands for the video.
/// In a single volume the chosen file's data must run past `head`, so a small
/// file (an nfo) whose header is the only one seen never stands in for a
/// video whose header lies beyond it. Keys under `min_size` are dropped too:
/// a sample or nfo spilling into the last volume after the video is the only
/// file continued there, and a wrong key would mark a real alt as other.
/// Only a video counts, so an inner archive (a RAR inside a RAR) gives none.
/// None for encrypted headers (`-hp`), keyed checks (RAR 5 `-p`), a file whose
/// CRC is not recorded, or a head with no last part of such a file.
#[cfg(test)]
pub fn rar_last_volume_key(head: &[u8], multi_volume: bool, min_size: u64) -> Option<InnerKey> {
    rar_last_volume_key_with(head, multi_volume, min_size, None)
}

/// [`rar_last_volume_key`] with the archive password from the copy's nzb
/// (F8): encrypted headers (`-hp`) are read with it. A check the password
/// keys (RAR 5 encrypted data) still gives no key: keyed values compare only
/// under one password.
// OPEN ITEM (zipfast): keyed checks (RAR 5 AES) as a key under one password,
// via zipfast::map::last_volume_identity once the pin is at 21474cf or later.
pub fn rar_last_volume_key_with(
    head: &[u8],
    multi_volume: bool,
    min_size: u64,
    password: Option<&str>,
) -> Option<InnerKey> {
    if !head.starts_with(b"Rar!") {
        return None;
    }
    let password = password.filter(|p| !p.is_empty());
    let map =
        MemberMap::from_volume(head, MapOptions::new("head.rar").password(password)).ok()?;
    // the volume's own main header must agree: a lone middle volume (names
    // too obfuscated to group) is no whole archive, and a set's last volume
    // is never its first
    if (map.header_encrypted && password.is_none())
        || !map.main_read
        || map.multi_volume != multi_volume
        || (multi_volume && map.first_volume)
    {
        return None;
    }
    map.members
        .iter()
        .filter(|m| {
            !m.is_dir
                && crate::streamrepair::is_stream_eligible(
                    m.name.as_deref().map_or(&m.raw_name[..], str::as_bytes),
                )
                && if multi_volume {
                    m.split_before
                } else {
                    m.packed.end > head.len() as u64
                }
        })
        .filter_map(|m| {
            let checks = m.checks?;
            if checks.keyed {
                return None;
            }
            Some(InnerKey {
                size: m.unpacked_size?,
                crc32: checks.crc32?,
            })
        })
        .max_by_key(|k| k.size)
        .filter(|k| k.size >= min_size)
}

/// Reads the header of a 7z set and returns its largest file's [`InnerKey`].
/// The volumes are `lens` bytes long; `read(volume, offset, buf)` fills `buf`
/// from a volume. Only header bytes are read (the start header and the
/// header near the end), so a set with a password but without `-mhe` gives
/// its key without the password. Errors say why there is none: not 7z,
/// encrypted headers, no CRC recorded, an unreadable header.
pub fn seven_zip_key<F>(lens: &[u64], mut read: F, first_name: &str) -> Result<InnerKey, String>
where
    F: FnMut(u32, u64, &mut [u8]) -> bool,
{
    if lens.is_empty() {
        return Err("no volumes".to_string());
    }
    let shared = Arc::new(Mutex::new(Wanted {
        lens: lens.to_vec(),
        want: None,
        have: (u32::MAX, 0, Vec::new()),
    }));
    let mut builder = MapBuilder::new(
        Box::new(Volumes(Arc::clone(&shared))),
        MapOptions::new(first_name),
    );
    let mut cx = Context::from_waker(Waker::noop());
    let map = loop {
        if let Poll::Ready(result) = builder.poll_map(&mut cx) {
            break result.map_err(|e| format!("7z header unreadable: {e:?}"))?;
        }
        let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
        let Some((vol, offset, len)) = s.want.take() else {
            return Err("7z header unreadable".to_string());
        };
        let vlen = s.lens.get(vol as usize).copied().unwrap_or(0);
        let len = len.min(usize::try_from(vlen.saturating_sub(offset)).unwrap_or(usize::MAX));
        let mut buf = vec![0u8; len];
        if len == 0 || !read(vol, offset, &mut buf) {
            return Err("7z header bytes could not be read".to_string());
        }
        s.have = (vol, offset, buf);
    };
    if map.format != MapFormat::SevenZip {
        return Err("not a 7z archive".to_string());
    }
    if map.header_encrypted {
        return Err("7z headers are encrypted (-mhe)".to_string());
    }
    let largest = map
        .members
        .iter()
        .filter(|m| !m.is_dir)
        .max_by_key(|m| m.unpacked_size.unwrap_or(0))
        .ok_or_else(|| "7z archive lists no files".to_string())?;
    let size = largest
        .unpacked_size
        .ok_or_else(|| "7z file size not recorded".to_string())?;
    let checks = largest
        .checks
        .filter(|c| !c.keyed)
        .ok_or_else(|| "7z file CRC not recorded".to_string())?;
    let crc32 = checks
        .crc32
        .ok_or_else(|| "7z file CRC not recorded".to_string())?;
    Ok(InnerKey { size, crc32 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/twinfields/fixtures");
    /// The payload every fixture packs: 49156 bytes, CRC32 0x771fbd29.
    const VIDEO: InnerKey = InnerKey {
        size: 49156,
        crc32: 0x771f_bd29,
    };

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{FIXTURES}/{name}")).unwrap()
    }

    /// A file cut into articles of `part` bytes: (offset, file size, bytes).
    fn articles(data: &[u8], part: usize) -> Vec<(i64, i64, Vec<u8>)> {
        data.chunks(part)
            .enumerate()
            .map(|(i, c)| ((i * part) as i64, data.len() as i64, c.to_vec()))
            .collect()
    }

    fn set(prefix: &str) -> Vec<Vec<u8>> {
        (1..=3)
            .map(|i| fixture(&format!("{prefix}.part{i}.rar")))
            .collect()
    }

    /// Runs a plan over two posts of the same file list and judges it.
    fn sample(a: &[Vec<u8>], pa: usize, b: &[Vec<u8>], pb: usize) -> SampleVerdict {
        let arts_a: Vec<_> = a.iter().map(|f| articles(f, pa)).collect();
        let arts_b: Vec<_> = b.iter().map(|f| articles(f, pb)).collect();
        let counts_a: Vec<usize> = arts_a.iter().map(Vec::len).collect();
        let counts_b: Vec<usize> = arts_b.iter().map(Vec::len).collect();
        let plan = if pa == pb {
            let bytes = |arts: &[Vec<(i64, i64, Vec<u8>)>]| -> Vec<i64> {
                arts.iter()
                    .flat_map(|f| f.iter().map(|x| x.2.len() as i64))
                    .collect()
            };
            plan_same_layout(
                &counts_a,
                &bytes(&arts_a),
                &counts_b,
                &bytes(&arts_b),
                MIDDLE_SAMPLES,
            )
        } else {
            plan_lcm(&counts_a, &counts_b, pa as u64, pb as u64, MIDDLE_SAMPLES)
        }
        .expect("a plan");
        let mut t = Tally {
            middle_planned: plan.iter().filter(|p| !p.header).count(),
            ..Tally::default()
        };
        for p in &plan {
            let x = &arts_a[p.file][p.seg_a];
            let y = &arts_b[p.file][p.seg_b];
            if p.file == 0 && p.seg_a == 0 {
                t.same_creation =
                    same_creation(creation_of(&x.2).as_ref(), creation_of(&y.2).as_ref());
            }
            t.add(
                p.header,
                pair_differs(p, (x.0, x.1, &x.2), (y.0, y.1, &y.2)),
            );
            if t.settled() {
                break;
            }
        }
        judge(&t, false, pa == pb)
    }

    #[test]
    fn a_rar_last_volume_keys_the_file_continued_into_it() {
        // video.mkv then info.nfo: the video's last part comes first
        let mm = rar_last_volume_key(&fixture("mm.part3.head"), true, 0).expect("video key");
        // info.nfo then video.mkv: still the video, not the nfo
        let nfo_first =
            rar_last_volume_key(&fixture("nfo-first.part3.head"), true, 0).expect("video key");
        // the fixture heads hold only the video's tail (the nfo lies beyond
        // 4096 bytes), so this checks the key, not the nfo being passed over;
        // the intake's majority rule backs up the split_before filter
        let video = InnerKey {
            size: 49156,
            crc32: 0x771f_bd29,
        };
        assert_eq!((mm, nfo_first), (video, video));
        // a first volume records no final check of a continued file
        assert_eq!(
            rar_last_volume_key(&fixture("mm.part1.head"), true, 0),
            None
        );
        // encrypted headers give no key
        assert_eq!(
            rar_last_volume_key(&fixture("hp1.part2.head"), true, 0),
            None
        );
        assert_eq!(rar_last_volume_key(b"not a rar", false, 0), None);
        // a key under the minimum size (a sample or nfo spilling over) is dropped
        let head = fixture("mm.part3.head");
        assert_eq!(rar_last_volume_key(&head, true, mm.size), Some(mm));
        assert_eq!(rar_last_volume_key(&head, true, mm.size + 1), None);
        // a set's volume read as if it were a whole archive gives no key
        assert_eq!(rar_last_volume_key(&head, false, 0), None);
        // a lone nfo whose data ends inside the head never stands for the
        // video whose header follows it (the name filter drops it; the
        // data-past-head rule has no fixture of its own, as no rar is at hand)
        let nfo_head = fixture("nfo-first.part1.head");
        let nfo = MemberMap::from_volume(&nfo_head, MapOptions::new("x.rar")).unwrap();
        assert!(
            nfo.members
                .iter()
                .any(|m| m.checks.is_some_and(|c| c.crc32.is_some())
                    && m.packed.end <= nfo_head.len() as u64),
            "{:?}",
            nfo.members.len()
        );
        assert_eq!(rar_last_volume_key(&nfo_head, false, 0), None);
    }

    #[test]
    fn identical_runs_are_twins() {
        let a = set("rerar-a");
        assert_eq!(sample(&a, 4096, &a, 4096), SampleVerdict::Twin);
    }

    #[test]
    fn an_mtime_rerar_differs_only_in_header_articles() {
        let (a, c) = (set("rerar-a"), set("rerar-c"));
        // measured: per volume only the first and the last article differ
        for (va, vc) in a.iter().zip(&c) {
            let (xa, xc) = (articles(va, 4096), articles(vc, 4096));
            let differ: Vec<usize> = (0..xa.len()).filter(|&i| xa[i].2 != xc[i].2).collect();
            assert_eq!(differ, vec![0, xa.len() - 1]);
        }
        assert_eq!(sample(&a, 4096, &c, 4096), SampleVerdict::Rerar { tail: 1 });
    }

    #[test]
    fn the_old_middle_only_sample_could_miss_every_header_article() {
        // 17 volumes of 143 articles (about 100 MB each): the six evenly
        // spaced articles of the old sample hit no volume's first or last
        // article, so a re-RAR looked like a twin; the new plan reads them
        let counts = vec![143usize; 17];
        let bytes = vec![716_800i64; 143 * 17];
        let old = crate::twin::sample_indices(&counts, &bytes, &counts, &bytes, 6).unwrap();
        assert!(old.iter().all(|&(_, k)| k != 0 && k != 142));
        let new = plan_same_layout(&counts, &bytes, &counts, &bytes, MIDDLE_SAMPLES).unwrap();
        assert!(new.iter().any(|p| p.header && p.seg_a == 0));
        assert!(new.iter().any(|p| p.header && p.seg_a == 142));
    }

    #[test]
    fn other_article_sizes_still_link_twins_and_rerars() {
        let (a, c) = (set("rerar-a"), set("rerar-c"));
        // 3000 and 4000 bytes: grid 12000, inside each 20480-byte volume once
        // other article counts: the same files, never a twin
        assert_eq!(sample(&a, 3000, &a, 4000), SampleVerdict::SameFiles);
        assert_eq!(sample(&a, 3000, &c, 4000), SampleVerdict::SameFiles);
        let p = fixture("pass.7z");
        let q = fixture("plain.7z");
        assert_eq!(
            sample(
                std::slice::from_ref(&p),
                3000,
                std::slice::from_ref(&q),
                4000
            ),
            SampleVerdict::Alt
        );
    }

    #[test]
    fn lcm_plan_reads_articles_that_start_together() {
        let plan = plan_lcm(&[100], &[94], 716_800, 768_000, 6).unwrap();
        for p in plan.iter().filter(|p| !p.header) {
            assert_eq!(p.seg_a as u64 * 716_800, p.seg_b as u64 * 768_000);
            // off the last eighth, where a recovery record sits
            assert!(p.seg_a > 0 && p.seg_a < middle_end(100) && p.seg_b < middle_end(94));
        }
        // only five grid points fall before the last eighth
        assert_eq!(plan.iter().filter(|p| !p.header).count(), 5);
        assert!(plan
            .iter()
            .any(|p| p.tail && p.seg_a == 99 && p.seg_b == 93));
        assert!(plan_lcm(&[100], &[100], 716_800, 716_800, 6).is_none());
        assert!(plan_lcm(&[100, 3], &[94], 716_800, 768_000, 6).is_none());
    }

    #[test]
    fn same_layout_plan_includes_header_articles_of_three_files() {
        let counts = [5usize, 5, 5, 5, 2];
        let bytes = vec![10i64; 22];
        let plan = plan_same_layout(&counts, &bytes, &counts, &bytes, 6).unwrap();
        let headers: Vec<(usize, usize)> = plan
            .iter()
            .filter(|p| p.header)
            .map(|p| (p.file, p.seg_a))
            .collect();
        assert_eq!(
            headers,
            vec![(0, 0), (0, 4), (2, 0), (2, 4), (4, 0), (4, 1)]
        );
        assert!(plan
            .iter()
            .filter(|p| !p.header)
            .all(|p| p.seg_a > 0 && p.seg_a + 1 < counts[p.file]));
        let mut other = bytes.clone();
        other[3] = 11;
        assert!(plan_same_layout(&counts, &bytes, &counts, &other, 6).is_none());
    }

    #[test]
    fn judge_needs_headers_and_enough_middles() {
        let mut t = Tally {
            middle_planned: 6,
            ..Tally::default()
        };
        assert_eq!(judge(&t, false, true), SampleVerdict::Retry);
        t.add(true, false);
        t.add(false, false);
        t.add(false, false);
        assert_eq!(judge(&t, false, true), SampleVerdict::Retry);
        t.add(false, false);
        assert_eq!(judge(&t, false, true), SampleVerdict::Twin);
        assert_eq!(judge(&t, false, false), SampleVerdict::SameFiles);
        assert_eq!(judge(&t, true, true), SampleVerdict::Retry);
        t.add(true, true);
        assert_eq!(judge(&t, false, true), SampleVerdict::Rerar { tail: 1 });
        assert_eq!(judge(&t, false, false), SampleVerdict::SameFiles);
        t.add(false, true);
        assert_eq!(judge(&t, false, true), SampleVerdict::Alt);
        // a header difference with no middle article to compare is no re-RAR
        let mut small = Tally::default();
        small.add(true, true);
        assert_eq!(judge(&small, false, true), SampleVerdict::Alt);
        let other_run = Tally {
            same_creation: Some(false),
            ..Tally::default()
        };
        assert_eq!(judge(&other_run, false, true), SampleVerdict::Alt);
    }

    #[test]
    fn rar5_hp_salt_is_one_per_run_and_shared_by_its_volumes() {
        let one = creation_of(&fixture("hp1.part1.head")).unwrap();
        assert_eq!(one.what, "RAR5 header salt");
        assert_eq!(one.bytes.len(), 16);
        let same_set = creation_of(&fixture("hp1.part2.head")).unwrap();
        let other_run = creation_of(&fixture("hp2.part1.head")).unwrap();
        assert_eq!(same_creation(Some(&one), Some(&same_set)), Some(true));
        assert_eq!(same_creation(Some(&one), Some(&other_run)), Some(false));
    }

    #[test]
    fn rar5_p_file_salt_and_iv_are_one_per_run() {
        let one = creation_of(&fixture("p1.part1.head")).unwrap();
        assert_eq!(one.what, "RAR5 file salt and IV");
        assert_eq!(one.bytes.len(), 32);
        let two = creation_of(&fixture("p2.part1.head")).unwrap();
        assert_eq!(same_creation(Some(&one), Some(&two)), Some(false));
        assert_eq!(same_creation(Some(&one), Some(&one.clone())), Some(true));
    }

    #[test]
    fn unencrypted_archives_have_no_creation_value() {
        assert_eq!(creation_of(&fixture("rerar-a.part1.rar")), None);
        assert_eq!(creation_of(&fixture("plain.7z")), None);
        assert_eq!(same_creation(None, None), None);
    }

    #[test]
    fn rar_set_ends_needs_one_set_and_nothing_else() {
        assert_eq!(
            rar_set_ends(&["x.part01.rar", "x.part02.rar", "x.part03.rar"]),
            Some((0, 2))
        );
        assert_eq!(rar_set_ends(&["X.PART1.RAR", "X.PART2.RAR"]), Some((0, 1)));
        // old naming sorts x.r00 first and x.rar last
        assert_eq!(rar_set_ends(&["x.r00", "x.r01", "x.rar"]), Some((2, 1)));
        assert_eq!(rar_set_ends(&["x.rar"]), Some((0, 0)));
        assert_eq!(rar_set_ends(&["x.part1.rar"]), Some((0, 0)));
        // other files, a gap, a second set, a duplicate number
        assert_eq!(rar_set_ends(&["x.nfo", "x.part1.rar", "x.part2.rar"]), None);
        assert_eq!(rar_set_ends(&["x.part1.rar", "x.part2.rar", "x.sfv"]), None);
        assert_eq!(rar_set_ends(&["x.part1.rar", "x.part3.rar"]), None);
        assert_eq!(
            rar_set_ends(&[
                "cd1.part1.rar",
                "cd1.part2.rar",
                "cd2.part1.rar",
                "cd2.part2.rar"
            ]),
            None
        );
        assert_eq!(rar_set_ends(&["x.r00", "x.r01"]), None);
        // a lone .rar of any name is a one-volume set; its headers decide
        assert_eq!(rar_set_ends(&["x.part0.rar"]), Some((0, 0)));
        assert_eq!(rar_set_ends(&[]), None);
    }

    #[test]
    fn rerar_lends_only_while_one_member_runs_through_the_set() {
        // one member: it runs from part 1 into part 3, ending before the
        // last volume's recovery record and end header
        let end = rerar_member_end(
            &fixture("rerar-a.part1.rar"),
            &fixture("rerar-a.part3.rar"),
            false,
        )
        .expect("one member");
        assert!(end > 0 && end < 8624);
        // video.mkv then info.nfo: the video runs through, the nfo header
        // (it differs in a re-RAR) sits after its end in part 3
        let end = rerar_member_end(&fixture("mm.part1.head"), &fixture("mm.part3.head"), false)
            .expect("the video runs through");
        assert!(
            end <= 8551,
            "nfo header bytes 8551.. differ in a re-RAR: {end}"
        );
        // info.nfo first: a member header lies inside part 1
        assert_eq!(
            rerar_member_end(
                &fixture("nfo-first.part1.head"),
                &fixture("nfo-first.part3.head"),
                false
            ),
            None
        );
        // the last volume must hold the end of the first volume's member
        assert_eq!(
            rerar_member_end(
                &fixture("rerar-a.part1.rar"),
                &fixture("rerar-a.part2.rar"),
                false
            ),
            None
        );
        assert_eq!(
            rerar_member_end(&fixture("plain.7z"), &fixture("plain.7z"), true),
            None
        );
    }

    #[test]
    fn middle_samples_stay_off_a_volume_end() {
        assert_eq!(middle_end(3), 2);
        assert_eq!(middle_end(7), 6);
        // 143 articles: the last 17 (a recovery record's place) are left out
        assert_eq!(middle_end(143), 125);
        let bytes = vec![700_000_i64; 143];
        let plan = plan_same_layout(&[143], &bytes, &[143], &bytes, 6).expect("plan");
        assert!(plan
            .iter()
            .filter(|p| !p.header)
            .all(|p| p.seg_a >= 1 && p.seg_a < 125));
    }

    #[test]
    fn sampled_relation_and_lending() {
        assert_eq!(
            sampled_relation(7, b"", b"", 9, b"rerar:2", b"7"),
            Some(SampleVerdict::Rerar { tail: 2 })
        );
        assert_eq!(sampled_relation(7, b"", b"", 9, b"rerar:0", b"7"), None);
        assert_eq!(
            sampled_relation(7, b"", b"", 9, b"samefiles", b"7"),
            Some(SampleVerdict::SameFiles)
        );
        assert_eq!(
            sampled_relation(7, b"twin:hdr", b"9", 9, b"", b""),
            Some(SampleVerdict::Twin)
        );
        // the older middle-only sampler's bare "twin" is not trusted
        assert_eq!(sampled_relation(7, b"twin", b"9", 9, b"", b""), None);
        assert_eq!(
            lend_rule(sampled_relation(7, b"twin", b"9", 9, b"", b""), 1, 30),
            Lend::Default
        );
        assert_eq!(
            sampled_relation(7, b"twin:hdr", b"8", 9, b"rerar", b"6"),
            None
        );
        assert_eq!(sampled_relation(7, b"none", b"9", 9, b"", b""), None);
        let rerar = Some(SampleVerdict::Rerar { tail: 1 });
        assert_eq!(lend_rule(rerar, 1, 30), Lend::Never);
        assert_eq!(lend_rule(rerar, 30, 30), Lend::Never);
        assert_eq!(lend_rule(rerar, 29, 30), Lend::LikeTwin);
        assert_eq!(lend_rule(rerar, 2, 30), Lend::LikeTwin);
        // a recovery record: the last two articles differ
        let rr = Some(SampleVerdict::Rerar { tail: 2 });
        assert_eq!(lend_rule(rr, 29, 30), Lend::Never);
        assert_eq!(lend_rule(rr, 28, 30), Lend::LikeTwin);
        assert_eq!(
            lend_rule(Some(SampleVerdict::SameFiles), 2, 30),
            Lend::Default
        );
        for v in [
            SampleVerdict::Twin,
            SampleVerdict::Rerar { tail: 3 },
            SampleVerdict::SameFiles,
            SampleVerdict::Alt,
        ] {
            assert_eq!(SampleVerdict::from_param(&v.param()), Some(v));
        }
        assert_eq!(lend_rule(Some(SampleVerdict::Twin), 1, 30), Lend::LikeTwin);
        assert_eq!(lend_rule(Some(SampleVerdict::Alt), 2, 30), Lend::Default);
        assert_eq!(lend_rule(None, 2, 30), Lend::Default);
    }

    fn key_of(vols: &[Vec<u8>], name: &str) -> Result<InnerKey, String> {
        let lens: Vec<u64> = vols.iter().map(|v| v.len() as u64).collect();
        seven_zip_key(
            &lens,
            |vol, off, buf| {
                let v = &vols[vol as usize];
                let (s, e) = (off as usize, off as usize + buf.len());
                if e > v.len() {
                    return false;
                }
                buf.copy_from_slice(&v[s..e]);
                true
            },
            name,
        )
    }

    #[test]
    fn seven_zip_password_without_mhe_gives_its_crc_without_the_password() {
        assert_eq!(key_of(&[fixture("pass.7z")], "rel.7z"), Ok(VIDEO));
        assert_eq!(key_of(&[fixture("plain.7z")], "rel.7z"), Ok(VIDEO));
        let split: Vec<Vec<u8>> = (1..=3)
            .map(|i| fixture(&format!("pass-split.7z.00{i}")))
            .collect();
        assert_eq!(key_of(&split, "rel.7z.001"), Ok(VIDEO));
        assert!(key_of(&[fixture("pass-mhe.7z")], "rel.7z").is_err());
        assert!(key_of(&[fixture("rerar-a.part1.rar")], "rel.part1.rar").is_err());
    }

    #[test]
    fn inner_keys_link_only_large_equal_files() {
        let big = InnerKey {
            size: MIN_KEY_SIZE,
            crc32: 1,
        };
        let p = big.param();
        assert_eq!(InnerKey::parse(p.as_bytes()), Some(big));
        assert_eq!(
            same_inner_key(p.as_bytes(), p.as_bytes(), MIN_KEY_SIZE),
            Some(true)
        );
        let other_crc = InnerKey { crc32: 2, ..big }.param();
        assert_eq!(
            same_inner_key(p.as_bytes(), other_crc.as_bytes(), MIN_KEY_SIZE),
            Some(false)
        );
        let other_size = InnerKey {
            size: big.size + HEADER_SIZED_DIFF + 1,
            ..big
        }
        .param();
        assert_eq!(
            same_inner_key(p.as_bytes(), other_size.as_bytes(), MIN_KEY_SIZE),
            Some(false)
        );
        let small = VIDEO.param();
        assert_eq!(
            same_inner_key(small.as_bytes(), small.as_bytes(), MIN_KEY_SIZE),
            None
        );
        assert_eq!(
            same_inner_key(small.as_bytes(), small.as_bytes(), 1),
            Some(true)
        );
        assert_eq!(same_inner_key(b"", p.as_bytes(), MIN_KEY_SIZE), None);
        // F11c: a header-sized difference (an mkv without its 11,245-byte
        // Tags tail) is a likely alt, never another file
        let short_tail = InnerKey {
            size: big.size - 11_245,
            crc32: 7,
        }
        .param();
        assert_eq!(same_inner_key(p.as_bytes(), short_tail.as_bytes(), MIN_KEY_SIZE), None);
        let far = InnerKey {
            size: big.size - HEADER_SIZED_DIFF - 1,
            crc32: 7,
        }
        .param();
        assert_eq!(same_inner_key(p.as_bytes(), far.as_bytes(), MIN_KEY_SIZE), Some(false));
        assert_eq!(InnerKey::parse(b"12:abc"), None);
    }

    #[test]
    fn overlap_compares_only_shared_bytes() {
        assert_eq!(overlap_equal(0, b"abcdef", 2, b"cdXY"), Some(false));
        assert_eq!(overlap_equal(0, b"abcdef", 2, b"cdef"), Some(true));
        assert_eq!(overlap_equal(0, b"ab", 2, b"cd"), None);
        let tail = SamplePair {
            file: 0,
            seg_a: 9,
            seg_b: 7,
            header: true,
            tail: true,
        };
        assert!(!pair_differs(&tail, (6, 10, b"ghij"), (8, 10, b"ij")));
        assert!(pair_differs(&tail, (6, 10, b"ghij"), (8, 11, b"ij")));
        let mid = SamplePair {
            tail: false,
            ..tail
        };
        assert!(pair_differs(&mid, (6, 10, b"ghij"), (8, 10, b"ij")));
    }
}
