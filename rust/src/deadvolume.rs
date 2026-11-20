//! Dead RAR volumes: volumes of a stored RAR set that arrived with no article at all.
//!
//! Such a volume leaves nothing on disk, not even its headers, so neither a hole skip nor an
//! in-place patch can reach it. Its place in the inner file can still be worked out: every
//! middle volume of a set is cut to the same size and carries one stored part of the same
//! member behind the same headers, so a sibling volume tells how many inner bytes the dead
//! one held, and the volumes before it tell where those bytes start.
//!
//! [`plan`] works out that inner range. Live stream repair then fetches the bytes from a
//! verified duplicate (a twin or an alt in any packing) into a scratch file, and
//! [`synthesize_frame`] rebuilds the headers around them from the sibling, so direct unpack
//! can read the volume as a virtual one. The archive on disk is never written: the rebuilt
//! headers and the borrowed data live outside it, and the extraction's own checks (the part
//! checksum computed over the borrowed bytes, the member checksum stored in the last volume)
//! still have to pass.

use std::path::{Path, PathBuf};

/// What is known about one volume of a set, in set order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VolumeState {
    /// On disk with readable headers.
    Readable(Geometry),
    /// No article arrived: nothing of it is on disk. `encoded_size` is what the nzb lists
    /// for it (0 when unknown).
    Dead { encoded_size: i64 },
    /// Not downloaded yet, or unreadable for another reason: its place is unknown.
    Unknown,
}

/// A readable volume's layout and the inner member's part in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// 3 for RAR 1.5 to 4, 5 for RAR5 (as `rarreader` reports it).
    pub version: i32,
    /// Size of the volume file.
    pub size: i64,
    /// Where the member's stored part starts in the volume.
    pub data_offset: i64,
    /// Length of that part.
    pub packed_size: i64,
    /// The part continues a previous volume and goes on in the next one.
    pub middle: bool,
}

impl Geometry {
    fn tail_len(&self) -> i64 {
        self.size - self.data_offset - self.packed_size
    }
}

/// Where a dead volume's bytes belong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The dead volume's position in the set.
    pub index: usize,
    /// The sibling volume its geometry and headers are taken from.
    pub template: usize,
    /// First inner byte the volume held.
    pub inner_offset: i64,
    /// Inner bytes it held.
    pub packed_size: i64,
    /// Where those bytes start in the volume (after its headers).
    pub data_offset: i64,
}

/// Bytes a RAR5 volume number takes in the main header (a vint).
fn rar5_volnum_len(index: usize) -> i64 {
    let mut v = index as u64;
    let mut n = 1;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

/// Plans every dead volume whose place can be proven from the volumes before it.
///
/// A dead volume is planned when it is neither the first nor the last one of the set, every
/// volume before it is readable or planned, and a readable middle sibling gives its geometry:
/// the nearest one before it and the nearest one after it (when there is one) must agree. In
/// RAR5 a volume's number is part of its main header, so siblings whose number takes as many
/// bytes are preferred; one from the other side of that boundary is shifted by the
/// difference. When the nzb lists the dead volume's size it has to fit the sibling's (yEnc
/// adds 1 to 4 percent), which rules out a short last volume. `inner_size` bounds every range.
pub fn plan(volumes: &[VolumeState], inner_size: i64) -> Vec<Plan> {
    plan_with_stop(volumes, inner_size).0
}

/// Why [`plan_with_stop`] stopped before the end of the set. Volumes are numbered from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The first volume has no readable headers.
    FirstNotReadable,
    /// The volume is not on disk, or not readable yet.
    NotReadable { volume: usize },
    /// No readable middle sibling gives the dead volume's layout.
    NoTemplate { volume: usize },
    /// The dead volume may be the set's (shorter) last one.
    MaybeLast { volume: usize },
    /// The nzb lists a size that does not fit the sibling's layout.
    SizeMismatch { volume: usize, encoded_size: i64, expected: i64 },
    /// The dead volume would end past the inner file.
    PastInnerEnd { volume: usize },
    /// The volumes up to this one hold more than the inner file.
    Overflow { volume: usize },
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::FirstNotReadable => f.write_str("volume 1 is not readable"),
            Self::NotReadable { volume } => {
                write!(f, "volume {volume} is not on disk or not readable yet")
            }
            Self::NoTemplate { volume } => {
                write!(f, "no readable sibling gives volume {volume}'s layout")
            }
            Self::MaybeLast { volume } => write!(f, "volume {volume} may be the set's last"),
            Self::SizeMismatch { volume, encoded_size, expected } => write!(
                f,
                "volume {volume} is listed at {encoded_size} bytes, which does not fit a \
                 {expected}-byte volume like its sibling"
            ),
            Self::PastInnerEnd { volume } => {
                write!(f, "volume {volume} would end past the inner file")
            }
            Self::Overflow { volume } => {
                write!(f, "the volumes up to {volume} hold more than the inner file")
            }
        }
    }
}

/// [`plan`], plus why planning stopped before the end of `volumes`, if it did. For the log.
pub fn plan_with_stop(volumes: &[VolumeState], inner_size: i64) -> (Vec<Plan>, Option<StopReason>) {
    let mut out = Vec::new();
    let Some(VolumeState::Readable(first)) = volumes.first() else {
        return (out, Some(StopReason::FirstNotReadable));
    };
    let last = volumes.len() - 1;
    let mut offset = first.packed_size;
    for (i, state) in volumes.iter().enumerate().skip(1) {
        let volume = i + 1;
        match state {
            VolumeState::Readable(g) => offset += g.packed_size,
            VolumeState::Unknown => return (out, Some(StopReason::NotReadable { volume })),
            VolumeState::Dead { encoded_size } => {
                let Some((template, g)) = template_for(volumes, i) else {
                    return (out, Some(StopReason::NoTemplate { volume }));
                };
                // The last volume known may be the set's last (shorter, closing the file): it
                // is planned only when the inner file goes on past it.
                if i == last && offset + g.packed_size >= inner_size {
                    return (out, Some(StopReason::MaybeLast { volume }));
                }
                let size = g.data_offset + g.packed_size + g.tail_len();
                let encoded_size = *encoded_size;
                if encoded_size > 0 && (encoded_size < size || encoded_size > size + size / 16) {
                    let stop = StopReason::SizeMismatch { volume, encoded_size, expected: size };
                    return (out, Some(stop));
                }
                if g.packed_size <= 0 || offset + g.packed_size > inner_size {
                    return (out, Some(StopReason::PastInnerEnd { volume }));
                }
                out.push(Plan {
                    index: i,
                    template,
                    inner_offset: offset,
                    packed_size: g.packed_size,
                    data_offset: g.data_offset,
                });
                offset += g.packed_size;
            }
        }
        if offset > inner_size {
            // Something before does not add up: plan nothing past it.
            return (out, Some(StopReason::Overflow { volume }));
        }
    }
    (out, None)
}

/// The geometry dead volume `i` would have, from its siblings, and the sibling it came from.
fn template_for(volumes: &[VolumeState], i: usize) -> Option<(usize, Geometry)> {
    let last = volumes.len() - 1;
    let middle = |j: usize| match &volumes[j] {
        VolumeState::Readable(g) if j != 0 && j != last && g.middle => Some(*g),
        _ => None,
    };
    let version = (1..last).find_map(middle)?.version;
    // In RAR5 the volume number's length moves the data start; RAR 1.5-4 have no number there.
    let shift = |j: usize| {
        if version == 5 {
            rar5_volnum_len(i) - rar5_volnum_len(j)
        } else {
            0
        }
    };
    let adjust = |j: usize, g: Geometry| Geometry {
        data_offset: g.data_offset + shift(j),
        packed_size: g.packed_size - shift(j),
        ..g
    };
    let same_class = |j: usize| shift(j) == 0;
    let before = (1..i)
        .rev()
        .find(|&j| middle(j).is_some_and(|g| g.version == version) && same_class(j));
    let after =
        (i + 1..last).find(|&j| middle(j).is_some_and(|g| g.version == version) && same_class(j));
    let pick = |j: usize| (j, adjust(j, middle(j).unwrap_or_else(|| unreachable!())));
    let chosen = match (before, after) {
        (Some(b), Some(a)) => {
            let (gb, ga) = (pick(b).1, pick(a).1);
            if gb != ga {
                return None;
            }
            pick(b)
        }
        (Some(b), None) => pick(b),
        (None, Some(a)) => pick(a),
        (None, None) => {
            // Only siblings on the other side of a RAR5 number-length boundary: shift one.
            let j = (1..i)
                .rev()
                .chain(i + 1..last)
                .find(|&j| middle(j).is_some_and(|g| g.version == version))?;
            pick(j)
        }
    };
    (chosen.1.packed_size > 0).then_some(chosen)
}

/// Name prefix of the scratch files beside the volumes that hold borrowed data.
pub const SCRATCH_PREFIX: &str = ".rustyget-deadvol-";

/// Scratch file name (in the download directory) for dead volume `volume_name`.
pub fn scratch_name(volume_name: &[u8]) -> Vec<u8> {
    let mut name = SCRATCH_PREFIX.as_bytes().to_vec();
    name.extend_from_slice(volume_name);
    name.extend_from_slice(b".data");
    name
}

/// True for a scratch file this module made.
pub fn is_scratch(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(SCRATCH_PREFIX) && n.ends_with(".data"))
}

/// A volume as the extraction reads it in place of a dead one: rebuilt headers around data
/// borrowed from a duplicate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VirtualVolume {
    pub head: Vec<u8>,
    pub data: PathBuf,
    pub data_len: u64,
    pub tail: Vec<u8>,
}

impl VirtualVolume {
    pub fn len(&self) -> u64 {
        self.head.len() as u64 + self.data_len + self.tail.len() as u64
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Why [`synthesize_frame`] or [`check_frame`] refused a rebuilt volume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// Only RAR 1.5-4 (3) and RAR5 (5) volumes can be rebuilt.
    UnsupportedVersion(i32),
    /// The sibling's headers cannot serve as a template.
    Sibling { format: &'static str, what: &'static str },
    /// The rebuilt headers do not read back with the header reader.
    ReadBack,
    /// The rebuilt headers read back without a file.
    NoFile,
    /// The rebuilt headers read back as something else than the expected part.
    Mismatch {
        filename: String,
        stored: bool,
        split_before: bool,
        split_after: bool,
        packed_size: i64,
        data_offset: i64,
    },
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion(v) => write!(f, "RAR version {v} cannot be rebuilt"),
            Self::Sibling { format, what } => write!(f, "{format} sibling headers: {what}"),
            Self::ReadBack => f.write_str("rebuilt headers do not read back"),
            Self::NoFile => f.write_str("rebuilt headers hold no file"),
            Self::Mismatch { filename, stored, split_before, split_after, packed_size, data_offset } => {
                write!(
                    f,
                    "rebuilt headers read back as {filename} (stored {stored}, split \
                     {split_before}/{split_after}, {packed_size} bytes at {data_offset})"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// Volume number `number` of sibling `template`, moved to volume `index`.
fn shifted_volume_number(number: u64, index: usize, template: usize) -> Option<u64> {
    number
        .checked_add(u64::try_from(index).ok()?)?
        .checked_sub(u64::try_from(template).ok()?)
}

/// Rebuilds the headers of dead volume `index` from those of sibling `template`.
///
/// `head` is the sibling's bytes in front of its stored part, `tail` the bytes behind it. The
/// result carries the dead volume's number, part length `packed` and part checksum
/// `part_crc` (CRC32 of the borrowed bytes), with the header checksums recomputed. Records
/// that point at other places in the sibling (RAR5's locator and quick-open data, RAR 1.5-4's
/// recovery record and archive checksum) are left out: they describe the sibling, and an
/// extraction does not need them. Encrypted parts and parts with another hash than CRC32 are
/// refused.
pub fn synthesize_frame(
    version: i32,
    head: &[u8],
    tail: &[u8],
    template: usize,
    index: usize,
    packed: u64,
    part_crc: u32,
) -> Result<(Vec<u8>, Vec<u8>), FrameError> {
    match version {
        5 => rar5::frame(head, tail, template, index, packed, part_crc),
        3 => rar4::frame(head, tail, template, index, packed, part_crc),
        v => Err(FrameError::UnsupportedVersion(v)),
    }
}

/// Checks a rebuilt volume with the regular header reader: it has to read as volume `index`
/// holding a stored middle part of `inner_name` that starts right after `head` and runs for
/// `packed` bytes.
pub fn check_frame(
    head: &[u8],
    tail: &[u8],
    packed: u64,
    inner_name: &[u8],
) -> Result<(), FrameError> {
    struct Frame<'a> {
        head: &'a [u8],
        packed: u64,
        tail: &'a [u8],
    }
    impl crate::rarreader::Source for Frame<'_> {
        fn size(&mut self) -> i64 {
            (self.head.len() as u64 + self.packed + self.tail.len() as u64) as i64
        }
        fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool {
            let Ok(mut at) = u64::try_from(offset) else {
                return false;
            };
            if at + buf.len() as u64 > self.size() as u64 {
                return false;
            }
            let head = self.head.len() as u64;
            for b in buf.iter_mut() {
                *b = if at < head {
                    self.head[at as usize]
                } else if at < head + self.packed {
                    0
                } else {
                    self.tail[(at - head - self.packed) as usize]
                };
                at += 1;
            }
            true
        }
    }
    let mut src = Frame { head, packed, tail };
    let mut vol = crate::rarreader::Volume::default();
    let mut files: Vec<crate::rarreader::RarFile> = Vec::new();
    let ok = crate::rarreader::read_from(&mut vol, &mut src, None, &mut files);
    if !ok && files.is_empty() {
        return Err(FrameError::ReadBack);
    }
    let Some(f) = files.first() else {
        return Err(FrameError::NoFile);
    };
    let base = |n: &[u8]| -> Vec<u8> {
        n.rsplit(|&b| b == b'/' || b == b'\\')
            .next()
            .unwrap_or(n)
            .to_vec()
    };
    if base(&f.filename) != base(inner_name)
        || !f.stored
        || f.encrypted_data
        || !f.split_before
        || !f.split_after
        || f.packed_size != packed as i64
        || f.data_offset != head.len() as i64
    {
        return Err(FrameError::Mismatch {
            filename: String::from_utf8_lossy(&f.filename).into_owned(),
            stored: f.stored,
            split_before: f.split_before,
            split_after: f.split_after,
            packed_size: f.packed_size,
            data_offset: f.data_offset,
        });
    }
    Ok(())
}

mod rar5 {
    use super::FrameError;

    const SIG: &[u8] = b"Rar!\x1a\x07\x01\x00";
    const HFL_EXTRA: u64 = 0x01;
    const HFL_DATA: u64 = 0x02;
    const HFL_SPLIT_BEFORE: u64 = 0x08;
    const HFL_SPLIT_AFTER: u64 = 0x10;
    const MHFL_VOLNUMBER: u64 = 0x02;
    const FHFL_MTIME: u64 = 0x02;
    const FHFL_CRC32: u64 = 0x04;
    const HEAD_MAIN: u64 = 1;
    const HEAD_FILE: u64 = 2;
    const HEAD_ENDARC: u64 = 5;
    const EXTRA_CRYPT: u64 = 1;
    const EXTRA_HASH: u64 = 2;

    pub(super) fn vint(b: &[u8], p: &mut usize) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let c = *b.get(*p)?;
            *p += 1;
            v |= u64::from(c & 0x7f) << shift;
            if c & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    pub(super) fn put_vint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let c = (v & 0x7f) as u8;
            v >>= 7;
            out.push(c | if v != 0 { 0x80 } else { 0 });
            if v == 0 {
                break;
            }
        }
    }

    /// One header block: its type, flags and the header bytes (from the type on), plus the
    /// length of the data area that follows it.
    struct Block<'a> {
        kind: u64,
        flags: u64,
        header: &'a [u8],
        data_len: u64,
        /// Offset of the first byte behind the type and flags in `header`.
        body: usize,
        extra_len: u64,
    }

    fn block<'a>(b: &'a [u8], p: &mut usize) -> Option<Block<'a>> {
        let start = *p;
        *p += 4; // header CRC
        let size = vint(b, p)? as usize;
        let h = b.get(*p..p.checked_add(size)?)?;
        let _ = start;
        *p += size;
        let mut q = 0;
        let kind = vint(h, &mut q)?;
        let flags = vint(h, &mut q)?;
        let extra_len = if flags & HFL_EXTRA != 0 {
            vint(h, &mut q)?
        } else {
            0
        };
        let data_len = if flags & HFL_DATA != 0 {
            vint(h, &mut q)?
        } else {
            0
        };
        Some(Block {
            kind,
            flags,
            header: h,
            data_len,
            body: q,
            extra_len,
        })
    }

    fn emit(out: &mut Vec<u8>, header: &[u8]) {
        let mut sized = Vec::with_capacity(header.len() + 3);
        put_vint(&mut sized, header.len() as u64);
        sized.extend_from_slice(header);
        out.extend_from_slice(&crc32fast::hash(&sized).to_le_bytes());
        out.extend_from_slice(&sized);
    }

    pub(super) fn frame(
        head: &[u8],
        tail: &[u8],
        template: usize,
        index: usize,
        packed: u64,
        part_crc: u32,
    ) -> Result<(Vec<u8>, Vec<u8>), FrameError> {
        let bad = |what: &'static str| FrameError::Sibling { format: "RAR5", what };
        if !head.starts_with(SIG) {
            return Err(bad("no signature"));
        }
        let mut out = SIG.to_vec();
        let mut p = SIG.len();
        let mut have_file = false;
        while p < head.len() {
            let b = block(head, &mut p).ok_or_else(|| bad("truncated header"))?;
            match b.kind {
                HEAD_MAIN => {
                    // Main header: the archive flags and the volume number; the extra area
                    // (locator of the sibling's quick-open and recovery data) is left out.
                    let mut q = b.body;
                    let aflags = vint(b.header, &mut q).ok_or_else(|| bad("main header"))?;
                    let mut h = Vec::new();
                    put_vint(&mut h, 1);
                    put_vint(&mut h, b.flags & !(HFL_EXTRA | HFL_DATA));
                    put_vint(&mut h, aflags);
                    if aflags & MHFL_VOLNUMBER != 0 {
                        let number = vint(b.header, &mut q).ok_or_else(|| bad("volume number"))?;
                        let number = super::shifted_volume_number(number, index, template)
                            .ok_or_else(|| bad("volume number"))?;
                        put_vint(&mut h, number);
                    } else {
                        return Err(bad("main header without a volume number"));
                    }
                    emit(&mut out, &h);
                    if b.data_len != 0 {
                        return Err(bad("main header with data"));
                    }
                }
                HEAD_FILE => {
                    if p != head.len() {
                        return Err(bad("file header does not end in front of its data"));
                    }
                    let h = b.header;
                    let extra_at = h
                        .len()
                        .checked_sub(b.extra_len as usize)
                        .ok_or_else(|| bad("extra area"))?;
                    let mut q = b.body;
                    let fflags = vint(h, &mut q).ok_or_else(|| bad("file flags"))?;
                    let fields_start = q;
                    let _unpacked = vint(h, &mut q).ok_or_else(|| bad("size"))?;
                    let _attr = vint(h, &mut q).ok_or_else(|| bad("attributes"))?;
                    if fflags & FHFL_MTIME != 0 {
                        q += 4;
                    }
                    let crc_at = (fflags & FHFL_CRC32 != 0).then_some(q);
                    if crc_at.is_some() {
                        q += 4;
                    }
                    let comp_info = vint(h, &mut q).ok_or_else(|| bad("compression info"))?;
                    if (comp_info >> 7) & 7 != 0 {
                        return Err(bad("compressed part"));
                    }
                    let rest = h
                        .get(fields_start..extra_at)
                        .ok_or_else(|| bad("file header"))?;
                    let extra = &h[extra_at..];
                    let mut e = 0;
                    while e < extra.len() {
                        let size = vint(extra, &mut e).ok_or_else(|| bad("extra record"))? as usize;
                        let rec_end = e
                            .checked_add(size)
                            .filter(|&x| x <= extra.len())
                            .ok_or_else(|| bad("extra record"))?;
                        let mut r = e;
                        let kind = vint(extra, &mut r).ok_or_else(|| bad("extra record"))?;
                        match kind {
                            EXTRA_CRYPT => return Err(bad("encrypted part")),
                            EXTRA_HASH => return Err(bad("part hash other than CRC32")),
                            _ => {}
                        }
                        e = rec_end;
                    }
                    let mut rebuilt = Vec::new();
                    put_vint(&mut rebuilt, 2);
                    let flags = (b.flags | HFL_DATA | HFL_SPLIT_BEFORE | HFL_SPLIT_AFTER)
                        & if extra.is_empty() {
                            !HFL_EXTRA
                        } else {
                            u64::MAX
                        };
                    put_vint(&mut rebuilt, flags);
                    if !extra.is_empty() {
                        put_vint(&mut rebuilt, extra.len() as u64);
                    }
                    put_vint(&mut rebuilt, packed);
                    put_vint(&mut rebuilt, fflags);
                    let mut fields = rest.to_vec();
                    if let Some(at) = crc_at {
                        let at = at - fields_start;
                        fields[at..at + 4].copy_from_slice(&part_crc.to_le_bytes());
                    }
                    rebuilt.extend_from_slice(&fields);
                    rebuilt.extend_from_slice(extra);
                    emit(&mut out, &rebuilt);
                    have_file = true;
                }
                _ => {
                    if b.data_len != 0 {
                        return Err(bad("unexpected data in front of the file"));
                    }
                    // Any other header in front of the part (none in usual sets) is kept.
                    emit(&mut out, b.header);
                }
            }
        }
        if !have_file {
            return Err(bad("no file header"));
        }
        // Behind the part: only the end-of-archive header is kept (it says another volume
        // follows); service headers such as quick-open describe the sibling.
        let mut p = 0;
        let mut end = None;
        while p < tail.len() {
            let Some(b) = block(tail, &mut p) else { break };
            if b.kind == HEAD_ENDARC {
                let mut e = Vec::new();
                emit(&mut e, b.header);
                end = Some(e);
                break;
            }
            p = p
                .checked_add(b.data_len as usize)
                .ok_or_else(|| bad("tail"))?;
        }
        let end = end.ok_or_else(|| bad("no end-of-archive header"))?;
        Ok((out, end))
    }
}

mod rar4 {
    use super::FrameError;

    const MARK: &[u8] = b"Rar!\x1a\x07\x00";
    const LONG_BLOCK: u16 = 0x8000;
    const LHD_SPLIT_BEFORE: u16 = 0x0001;
    const LHD_SPLIT_AFTER: u16 = 0x0002;
    const LHD_PASSWORD: u16 = 0x0004;
    const LHD_LARGE: u16 = 0x0100;
    const EARC_DATACRC: u16 = 0x0002;
    const EARC_REVSPACE: u16 = 0x0004;
    const EARC_VOLNUMBER: u16 = 0x0008;
    const FILE_HEAD: u8 = 0x74;
    const ENDARC_HEAD: u8 = 0x7b;
    /// Method byte of a stored (uncompressed) file.
    const METHOD_STORE: u8 = 0x30;

    fn u16le(b: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
    }
    fn u32le(b: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
    }

    /// Sets the 16-bit header checksum (the low half of the CRC32 behind it).
    fn seal(block: &mut [u8]) {
        let crc = (crc32fast::hash(&block[2..]) & 0xffff) as u16;
        block[..2].copy_from_slice(&crc.to_le_bytes());
    }

    pub(super) fn frame(
        head: &[u8],
        tail: &[u8],
        template: usize,
        index: usize,
        packed: u64,
        part_crc: u32,
    ) -> Result<(Vec<u8>, Vec<u8>), FrameError> {
        let bad = |what: &'static str| FrameError::Sibling { format: "RAR4", what };
        if !head.starts_with(MARK) {
            return Err(bad("no marker"));
        }
        let mut out = MARK.to_vec();
        let mut p = MARK.len();
        let mut have_file = false;
        while p < head.len() {
            let kind = *head.get(p + 2).ok_or_else(|| bad("truncated header"))?;
            let flags = u16le(head, p + 3).ok_or_else(|| bad("truncated header"))?;
            let size = u16le(head, p + 5).ok_or_else(|| bad("truncated header"))? as usize;
            if size < 7 {
                return Err(bad("short header"));
            }
            let mut block = head
                .get(p..p + size)
                .ok_or_else(|| bad("truncated header"))?
                .to_vec();
            match kind {
                FILE_HEAD => {
                    if p + size != head.len() {
                        return Err(bad("file header does not end in front of its data"));
                    }
                    if flags & LHD_PASSWORD != 0 {
                        return Err(bad("encrypted part"));
                    }
                    if block.get(25).copied() != Some(METHOD_STORE) {
                        return Err(bad("compressed part"));
                    }
                    let mut f = flags | LHD_SPLIT_BEFORE | LHD_SPLIT_AFTER;
                    if packed > u64::from(u32::MAX) && flags & LHD_LARGE == 0 {
                        return Err(bad("part too large for the sibling's header"));
                    }
                    f |= flags & LHD_LARGE;
                    block[3..5].copy_from_slice(&f.to_le_bytes());
                    block[7..11].copy_from_slice(&(packed as u32).to_le_bytes());
                    block[16..20].copy_from_slice(&part_crc.to_le_bytes());
                    if flags & LHD_LARGE != 0 {
                        if block.len() < 40 {
                            return Err(bad("large file header"));
                        }
                        block[32..36].copy_from_slice(&((packed >> 32) as u32).to_le_bytes());
                    }
                    seal(&mut block);
                    out.extend_from_slice(&block);
                    have_file = true;
                    p += size;
                }
                _ => {
                    // Main header and anything else in front of the part, as the sibling has it.
                    let add = if flags & LONG_BLOCK != 0 {
                        u32le(head, p + 7).ok_or_else(|| bad("truncated header"))? as usize
                    } else {
                        0
                    };
                    if add != 0 {
                        return Err(bad("unexpected data in front of the file"));
                    }
                    out.extend_from_slice(&block);
                    p += size;
                }
            }
        }
        if !have_file {
            return Err(bad("no file header"));
        }
        // Behind the part: only the end-of-archive block, without the sibling's archive
        // checksum and with this volume's number.
        let mut p = 0;
        while p + 7 <= tail.len() {
            let kind = tail[p + 2];
            let flags = u16le(tail, p + 3).ok_or_else(|| bad("tail"))?;
            let size = u16le(tail, p + 5).ok_or_else(|| bad("tail"))? as usize;
            if size < 7 {
                break;
            }
            if kind == ENDARC_HEAD {
                let mut q = p + 7;
                if flags & EARC_DATACRC != 0 {
                    q += 4;
                }
                let mut end = Vec::new();
                end.extend_from_slice(&[0, 0, ENDARC_HEAD]);
                // Without the reserved space too: readers check the block's length against
                // its flags.
                let new_flags = flags & !(EARC_DATACRC | EARC_REVSPACE);
                end.extend_from_slice(&new_flags.to_le_bytes());
                end.extend_from_slice(&[0, 0]);
                if flags & EARC_VOLNUMBER != 0 {
                    let n = u16le(tail, q).ok_or_else(|| bad("end block"))?;
                    let n = super::shifted_volume_number(u64::from(n), index, template)
                        .and_then(|n| u16::try_from(n).ok())
                        .ok_or_else(|| bad("volume number"))?;
                    end.extend_from_slice(&n.to_le_bytes());
                }
                let len = end.len() as u16;
                end[5..7].copy_from_slice(&len.to_le_bytes());
                seal(&mut end);
                return Ok((out, end));
            }
            let add = if flags & LONG_BLOCK != 0 {
                u32le(tail, p + 7).unwrap_or(0) as usize
            } else {
                0
            };
            p += size + add;
        }
        Err(bad("no end-of-archive block"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(size: i64, at: i64, packed: i64) -> VolumeState {
        VolumeState::Readable(Geometry {
            version: 5,
            size,
            data_offset: at,
            packed_size: packed,
            middle: true,
        })
    }
    fn first(size: i64, at: i64, packed: i64) -> VolumeState {
        VolumeState::Readable(Geometry {
            version: 5,
            size,
            data_offset: at,
            packed_size: packed,
            middle: false,
        })
    }
    const DEAD: VolumeState = VolumeState::Dead { encoded_size: 0 };

    #[test]
    fn plans_dead_middle_volumes_from_siblings() {
        let v = vec![
            first(1000, 60, 900),
            g(1000, 61, 899),
            DEAD,
            DEAD,
            g(1000, 61, 899),
            g(500, 61, 300),
        ];
        let p = plan(&v, 900 + 899 * 4 + 300);
        assert_eq!(
            p,
            vec![
                Plan {
                    index: 2,
                    template: 1,
                    inner_offset: 1799,
                    packed_size: 899,
                    data_offset: 61
                },
                Plan {
                    index: 3,
                    template: 1,
                    inner_offset: 2698,
                    packed_size: 899,
                    data_offset: 61
                },
            ]
        );
    }

    #[test]
    fn stops_at_unknown_and_never_plans_first_or_a_closing_last() {
        let v = vec![
            first(1000, 60, 900),
            g(1000, 61, 899),
            VolumeState::Unknown,
            DEAD,
            g(1000, 61, 899),
            g(10, 1, 1),
        ];
        assert!(plan(&v, 1 << 40).is_empty());
        // A dead volume last of those known: planned only while the file goes on past it.
        let v = vec![first(1000, 60, 900), g(1000, 61, 899), DEAD];
        assert!(plan(&v, 900 + 899 + 899).is_empty());
        assert!(plan(&v, 900 + 899 + 500).is_empty());
        assert_eq!(
            plan(&v, 900 + 899 + 900),
            vec![Plan {
                index: 2,
                template: 1,
                inner_offset: 1799,
                packed_size: 899,
                data_offset: 61
            }]
        );
        let v = vec![DEAD, g(1000, 61, 899), g(1000, 61, 899)];
        assert!(plan(&v, 1 << 40).is_empty());
    }

    #[test]
    fn disagreeing_siblings_plan_nothing() {
        let v = vec![
            first(1000, 60, 900),
            g(1000, 61, 899),
            DEAD,
            g(1100, 61, 999),
            g(10, 1, 1),
        ];
        assert!(plan(&v, 1 << 40).is_empty());
    }

    #[test]
    fn nzb_size_must_fit_the_sibling() {
        let short = VolumeState::Dead { encoded_size: 400 };
        let v = vec![first(1000, 60, 900), g(1000, 61, 899), short, g(10, 1, 1)];
        assert!(plan(&v, 1 << 40).is_empty());
        let ok = VolumeState::Dead { encoded_size: 1020 };
        let v = vec![first(1000, 60, 900), g(1000, 61, 899), ok, g(10, 1, 1)];
        assert_eq!(plan(&v, 1 << 40).len(), 1);
    }

    #[test]
    fn rar5_volume_number_boundary_shifts_the_data() {
        // Volumes 1..=127 take one byte for their number, 128 on take two.
        let mut v = vec![first(1000, 60, 900)];
        for _ in 1..128 {
            v.push(g(1000, 61, 899));
        }
        v.push(DEAD); // 128
        v.push(g(1000, 62, 898)); // 129
        v.push(g(10, 1, 1));
        let p = plan(&v, 1 << 40);
        assert_eq!(p.len(), 1);
        assert_eq!(
            (
                p[0].index,
                p[0].template,
                p[0].data_offset,
                p[0].packed_size
            ),
            (128, 129, 62, 898)
        );
        // Without a two-byte sibling yet, a one-byte one is shifted.
        v[129] = VolumeState::Unknown;
        let p = plan(&v, 1 << 40);
        assert_eq!(
            (p[0].template, p[0].data_offset, p[0].packed_size),
            (127, 62, 898)
        );
    }

    fn testdata(name: &str) -> Vec<u8> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/testdata")
            .join(name);
        std::fs::read(p).unwrap()
    }

    fn parse(bytes: &[u8]) -> (crate::rarreader::Volume, Vec<crate::rarreader::RarFile>) {
        struct Mem<'a>(&'a [u8]);
        impl crate::rarreader::Source for Mem<'_> {
            fn size(&mut self) -> i64 {
                self.0.len() as i64
            }
            fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool {
                let o = offset as usize;
                match self.0.get(o..o + buf.len()) {
                    Some(s) => {
                        buf.copy_from_slice(s);
                        true
                    }
                    None => false,
                }
            }
        }
        let mut vol = crate::rarreader::Volume::default();
        let mut files = Vec::new();
        crate::rarreader::read_from(&mut vol, &mut Mem(bytes), None, &mut files);
        (vol, files)
    }

    /// Store-mode middle volume made from the RAR5 fixture's middle volume, method patched to
    /// stored (the fixture is compressed; headers are otherwise what WinRAR writes).
    fn stored_rar5_middle() -> (Vec<u8>, usize, usize) {
        let mut b = testdata("rarrenamer/testfile5.part02.rar");
        let (_, files) = parse(&b);
        let f = &files[0];
        let at = f.data_offset as usize;
        let len = f.packed_size as usize;
        // Rewrite the file header with comp info 0 (stored) and fix its CRC.
        let mut p = 8;
        // main header
        let mut q = p + 4;
        let size = rar5::vint(&b, &mut q).unwrap() as usize;
        p = q + size;
        let hdr_start = p;
        let mut q = p + 4;
        let size = rar5::vint(&b, &mut q).unwrap() as usize;
        let body = q;
        let mut r = body;
        rar5::vint(&b, &mut r).unwrap(); // type
        let hflags = rar5::vint(&b, &mut r).unwrap();
        if hflags & 1 != 0 {
            rar5::vint(&b, &mut r).unwrap(); // extra size
        }
        rar5::vint(&b, &mut r).unwrap(); // data size
        let ff = rar5::vint(&b, &mut r).unwrap();
        rar5::vint(&b, &mut r).unwrap();
        rar5::vint(&b, &mut r).unwrap();
        if ff & 2 != 0 {
            r += 4;
        }
        if ff & 4 != 0 {
            r += 4;
        }
        // comp info is a 2-byte vint here (0x180 -> method 3); keep its length, zero method.
        assert_eq!(b[r] & 0x80, 0x80);
        b[r] = 0x80;
        b[r + 1] = 0x00;
        let mut sized = Vec::new();
        rar5::put_vint(&mut sized, size as u64);
        sized.extend_from_slice(&b[body..body + size]);
        let crc = crc32fast::hash(&sized);
        b[hdr_start..hdr_start + 4].copy_from_slice(&crc.to_le_bytes());
        (b, at, len)
    }

    #[test]
    fn rar5_frame_reads_back_as_the_dead_volume() {
        let (b, at, len) = stored_rar5_middle();
        let (head, tail) = (&b[..at], &b[at + len..]);
        let data: Vec<u8> = (0..len as u32).map(|i| (i * 7) as u8).collect();
        let crc = crc32fast::hash(&data);
        // Sibling is volume 1 (number 1); rebuild volume 5.
        let (h, t) = synthesize_frame(5, head, tail, 1, 5, len as u64, crc).unwrap();
        check_frame(&h, &t, len as u64, b"testfile5.dat").unwrap();
        let mut whole = h.clone();
        whole.extend_from_slice(&data);
        whole.extend_from_slice(&t);
        let (vol, files) = parse(&whole);
        assert_eq!(vol.volume_no, 5);
        assert!(vol.has_next_volume);
        assert_eq!(files[0].packed_size as usize, len);
        assert_eq!(files[0].data_offset as usize, h.len());
        // Locator dropped: one byte shorter main header is fine as long as it reads.
        assert!(h.len() <= head.len());
        // The part checksum is the borrowed bytes' CRC32.
        let mut found = false;
        for w in h.windows(4) {
            found |= w == crc.to_le_bytes();
        }
        assert!(found);
    }

    #[test]
    fn rar5_frame_refuses_compressed_parts() {
        let b = testdata("rarrenamer/testfile5.part02.rar");
        let (_, files) = parse(&b);
        let at = files[0].data_offset as usize;
        let len = files[0].packed_size as usize;
        let err = synthesize_frame(5, &b[..at], &b[at + len..], 1, 2, len as u64, 0).unwrap_err();
        assert_eq!(err, FrameError::Sibling { format: "RAR5", what: "compressed part" });
    }

    #[test]
    fn rar4_frame_reads_back_as_the_dead_volume() {
        let b = testdata("rarrenamer/testfile3.part02.rar");
        let (_, files) = parse(&b);
        let f = &files[0];
        let at = f.data_offset as usize;
        let len = f.packed_size as usize;
        let mut head = b[..at].to_vec();
        // Make the fixture's part stored (method byte 0x30) so it qualifies.
        let fh = head.len() - (head.len() - 7 - 13);
        let file_hdr = 7 + 13; // marker + main header in these fixtures
        assert_eq!(head[file_hdr + 2], 0x74, "fixture layout {fh}");
        head[file_hdr + 25] = 0x30;
        let data = vec![0xabu8; len];
        let crc = crc32fast::hash(&data);
        let (h, t) = synthesize_frame(3, &head, &b[at + len..], 1, 4, len as u64, crc).unwrap();
        check_frame(&h, &t, len as u64, &f.filename).unwrap();
        let mut whole = h.clone();
        whole.extend_from_slice(&data);
        whole.extend_from_slice(&t);
        let (_, files) = parse(&whole);
        assert_eq!(files[0].packed_size as usize, len);
        assert!(files[0].split_before && files[0].split_after);
    }
}
