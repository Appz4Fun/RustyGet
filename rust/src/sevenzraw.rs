//! Borrowing bytes from a 7z duplicate whose LZMA2 coder could not compress the data.
//!
//! LZMA2 stores data it cannot shrink (video, audio, already packed files) as *raw chunks*:
//! a three byte header (`0x01` or `0x02`, then the payload size minus one, big endian)
//! followed by the payload itself. The inner file's bytes are therefore present in the
//! archive in plain form, only interleaved with chunk headers at positions that depend on
//! the data. There is no index of the chunks, so the place of an inner byte in the packed
//! stream cannot be computed from the headers alone.
//!
//! [`Lzma2RawSource`] finds it locally instead. Near the position where the wanted bytes
//! should be it looks for a run of chunk headers that chain into one another, then asks the
//! caller for the bytes the *target* already has around that inner position and searches the
//! chunk payload among them. One match fixes the alignment (a *sync point*); from there the
//! chunks are walked forward and their payloads returned. Only the few articles around the
//! hole are ever read. A chunk that really was compressed ends the walk: such a stretch is
//! left to the other repair paths.
//!
//! The LZMA2 stream itself is read with zipfast (`zipfast::map`): its chain finder
//! (`find_stored_chunk_chains`) proposes chunk starts, and from a sync point an
//! `Lzma2Index` resumed there walks the chunks and plans which packed bytes hold the wanted
//! output. What stays here is the part only a repair has: fixing the output position of a
//! chunk by finding its payload among the target's own bytes.

use std::cell::RefCell;

/// Links of chunk headers that must chain before a candidate is believed.
const CHAIN_LINKS: usize = 3;
/// Payload bytes searched in the target to fix the alignment.
const SAMPLE_LEN: usize = 48;
/// Target bytes read either side of the estimated inner position.
const SEARCH_RADIUS: i64 = 256 * 1024;
/// Packed bytes read per slab while looking for a chunk chain or walking it.
const SLAB: usize = 512 * 1024;
/// Farthest a read may start behind a sync point (walked chunk by chunk).
const MAX_WALK: i64 = 64 * 1024 * 1024;
/// Bytes compared beyond the sample before a sync point is accepted.
const CONFIRM_LEN: usize = 4096;

/// Where the inner file lives: see `SevenZipLzma2Layout` in `streamrepair`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lzma2RawLayout {
    pub inner_size: i64,
    pub inner_offset_in_folder: i64,
    pub folder_unpack_size: i64,
    pub pack_size: i64,
}

impl Lzma2RawLayout {
    /// True when the packed stream is nearly as large as its content (90 percent or more):
    /// LZMA2 gave up on most of it and stored raw chunks. Real remuxes measured 94.7 to 99.99
    /// percent: raw video with a few compressed stretches (headers, padding). Borrowing works
    /// wherever the chunks around a hole are raw; a compressed chunk in the way ends it.
    pub fn looks_raw(&self) -> bool {
        self.folder_unpack_size > 0
            && (self.pack_size as i128) * 10 >= (self.folder_unpack_size as i128) * 9
    }
}

/// One fixed alignment: the chunk whose header sits at `packed` (relative to the start of
/// the folder's packed stream) carries the folder's unpacked bytes from `unpacked` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncPoint {
    unpacked: i64,
    packed: i64,
}

/// Where chains of stored chunk headers start in `data` (a window at unknown alignment).
fn find_chunk_chains(data: &[u8]) -> Vec<usize> {
    zipfast::map::find_stored_chunk_chains(data, CHAIN_LINKS)
}

/// The LZMA2 chunks in `packed` (a window whose byte 0 is a chunk header at output offset
/// `unpacked`), as zipfast reads them: a resumed walk, ended by the window's end, damage or
/// the end marker.
fn index_from(packed: &[u8], unpacked: i64) -> zipfast::map::Lzma2Index {
    zipfast::map::Lzma2Index::from_bytes(
        packed,
        zipfast::map::Lzma2IndexOptions {
            start: zipfast::map::Lzma2Start::Resume(zipfast::map::Lzma2Position {
                packed: 0,
                unpacked: unpacked.max(0) as u64,
            }),
            ..Default::default()
        },
    )
}

/// The payload of the stored chunk whose header starts `data`, if it is one.
fn stored_payload(data: &[u8]) -> Option<&[u8]> {
    let index = index_from(data, 0);
    let first = index.chunks.first()?;
    if first.kind != zipfast::map::Lzma2ChunkKind::Stored {
        return None;
    }
    let p = first.payload();
    data.get(p.start as usize..(p.end as usize).min(data.len()))
}

fn has_varied_bytes(data: &[u8]) -> bool {
    let mut seen = [false; 256];
    let mut distinct = 0;
    for &b in data {
        if !seen[usize::from(b)] {
            seen[usize::from(b)] = true;
            distinct += 1;
            if distinct >= 8 {
                return true;
            }
        }
    }
    false
}

/// Every position of `needle` in `hay`.
fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || hay.len() < needle.len() {
        return out;
    }
    let first = needle[0];
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        match hay[i..=hay.len() - needle.len()]
            .iter()
            .position(|&b| b == first)
        {
            Some(rel) => {
                let at = i + rel;
                if &hay[at..at + needle.len()] == needle {
                    out.push(at);
                }
                i = at + 1;
            }
            None => break,
        }
    }
    out
}

/// The inner file of an LZMA2 raw-chunk 7z donor, readable near established sync points.
///
/// `read_packed(offset, buf)` reads the folder's packed stream (offset 0 is its first byte).
pub struct Lzma2RawSource<'a> {
    layout: Lzma2RawLayout,
    read_packed: Box<dyn Fn(i64, &mut [u8]) -> bool + 'a>,
    syncs: RefCell<Vec<SyncPoint>>,
    packed_bytes_read: RefCell<i64>,
}

impl<'a> Lzma2RawSource<'a> {
    pub fn new(
        layout: Lzma2RawLayout,
        read_packed: Box<dyn Fn(i64, &mut [u8]) -> bool + 'a>,
    ) -> Self {
        Self {
            layout,
            read_packed,
            syncs: RefCell::new(Vec::new()),
            packed_bytes_read: RefCell::new(0),
        }
    }

    pub fn inner_size(&self) -> i64 {
        self.layout.inner_size
    }

    fn read_packed_clamped(&self, offset: i64, len: usize) -> Option<Vec<u8>> {
        if offset < 0 || offset >= self.layout.pack_size {
            return None;
        }
        let len = len.min((self.layout.pack_size - offset) as usize);
        let mut buf = vec![0u8; len];
        if !(self.read_packed)(offset, &mut buf) {
            return None;
        }
        *self.packed_bytes_read.borrow_mut() += len as i64;
        Some(buf)
    }

    fn add_sync(&self, sp: SyncPoint) {
        let mut syncs = self.syncs.borrow_mut();
        match syncs.binary_search_by_key(&sp.unpacked, |s| s.unpacked) {
            Ok(_) => {}
            Err(i) => syncs.insert(i, sp),
        }
    }

    /// Establishes a sync point shortly before inner offset `inner_before` (normally the
    /// start of a hole). `read_target(inner_offset, buf)` returns bytes the target already
    /// has; `target_valid(inner_offset, len)` says whether that range is free of holes.
    /// Returns true when the alignment was found and confirmed.
    pub fn sync_before(
        &self,
        inner_before: i64,
        read_target: &mut dyn FnMut(i64, &mut [u8]) -> bool,
        target_valid: &dyn Fn(i64, i64) -> bool,
    ) -> bool {
        let l = &self.layout;
        if inner_before < 0 || inner_before > l.inner_size || l.folder_unpack_size <= 0 {
            return false;
        }
        let want_unpacked = l.inner_offset_in_folder + inner_before;
        if self
            .syncs
            .borrow()
            .iter()
            .any(|s| s.unpacked <= want_unpacked && want_unpacked - s.unpacked <= SLAB as i64)
        {
            return true;
        }
        // Raw storage makes the packed stream slightly larger than its content, so the packed
        // position of an unpacked offset is that offset scaled by the overall ratio.
        let est_packed = ((want_unpacked as i128) * (l.pack_size as i128)
            / (l.folder_unpack_size as i128)) as i64;
        // Look in slabs that end at (and then move back from) the estimate.
        for back in 0..4i64 {
            let slab_start = (est_packed - (SLAB as i64) * (back + 1) + (SLAB as i64) / 8).max(0);
            let Some(slab) = self.read_packed_clamped(slab_start, SLAB) else {
                return false;
            };
            for chain_pos in find_chunk_chains(&slab) {
                let Some(payload) = stored_payload(&slab[chain_pos..]) else {
                    continue;
                };
                if payload.len() < SAMPLE_LEN {
                    continue;
                }
                let sample = &payload[..SAMPLE_LEN];
                if !has_varied_bytes(sample) {
                    continue;
                }
                let packed = slab_start + chain_pos as i64;
                // Estimated inner offset of this payload, from the same ratio.
                let est_unpacked = ((packed as i128) * (l.folder_unpack_size as i128)
                    / (l.pack_size as i128)) as i64;
                let est_inner = est_unpacked - l.inner_offset_in_folder;
                let win_start = (est_inner - SEARCH_RADIUS).max(0);
                let win_end = (est_inner + SEARCH_RADIUS).min(l.inner_size);
                if win_end - win_start < SAMPLE_LEN as i64 {
                    continue;
                }
                let mut window = vec![0u8; (win_end - win_start) as usize];
                if !read_target(win_start, &mut window) {
                    continue;
                }
                let hits: Vec<usize> = find_all(&window, sample)
                    .into_iter()
                    .filter(|&h| target_valid(win_start + h as i64, SAMPLE_LEN as i64))
                    .collect();
                if hits.len() != 1 {
                    continue;
                }
                let inner_at = win_start + hits[0] as i64;
                if inner_at > inner_before {
                    // A sync point must lie before the bytes it is meant to reach.
                    continue;
                }
                // Confirm on more of the payload where the target has it.
                let confirm = payload.len().min(CONFIRM_LEN);
                let mut have = vec![0u8; confirm];
                if inner_at + confirm as i64 > l.inner_size
                    || !target_valid(inner_at, confirm as i64)
                    || !read_target(inner_at, &mut have)
                    || have != payload[..confirm]
                {
                    continue;
                }
                self.add_sync(SyncPoint {
                    unpacked: l.inner_offset_in_folder + inner_at,
                    packed,
                });
                return true;
            }
        }
        false
    }

    /// Reads inner bytes by walking raw chunks forward from the nearest earlier sync point.
    /// Fails without one, or when a compressed chunk lies in the way.
    pub fn read_inner(&self, inner_offset: i64, buf: &mut [u8]) -> bool {
        let l = &self.layout;
        if inner_offset < 0 || inner_offset + buf.len() as i64 > l.inner_size {
            return false;
        }
        if buf.is_empty() {
            return true;
        }
        let want_start = l.inner_offset_in_folder + inner_offset;
        let want_end = want_start + buf.len() as i64;
        let start = {
            let syncs = self.syncs.borrow();
            let idx = match syncs.binary_search_by_key(&want_start, |s| s.unpacked) {
                Ok(i) => Some(i),
                Err(0) => None,
                Err(i) => Some(i - 1),
            };
            match idx.map(|i| syncs[i]) {
                Some(sp) if want_start - sp.unpacked <= MAX_WALK => sp,
                _ => return false,
            }
        };

        // Walk forward from the sync point in slabs, as zipfast indexes what has been read.
        // Only stored chunks are accepted between the sync point and the end of the wanted
        // bytes: a compressed chunk (or damage, or the end marker) there ends the borrow, so a
        // misplaced sync point can never be carried across LZMA data onto the wrong bytes.
        // Chunks fully behind the wanted bytes are dropped from the window as the walk goes,
        // and every MiB of them leaves a sync point, so memory stays small and a later read
        // (or a retry after a failure) starts close.
        // The most a stored chunk's header and payload take: one chunk may run past the end.
        const CHUNK_MAX: i64 = 65_536 + 3;
        let mut base = start;
        let mut slab: Vec<u8> = Vec::new();
        let mut last_noted = start.unpacked;
        // Packed bytes the next step reads: at most a slab, and only what the wanted bytes can
        // still need (their output, headers, and one chunk running past the end).
        let need = want_end - start.unpacked;
        let mut step = need + need / 16_384 + CHUNK_MAX + 64;
        loop {
            if base.packed + slab.len() as i64 >= l.pack_size && slab.is_empty() {
                return false;
            }
            if base.packed + slab.len() as i64 >= l.pack_size {
                // Nothing more to read: the index below decides.
            } else {
                let Some(more) = self.read_packed_clamped(
                    base.packed + slab.len() as i64,
                    usize::try_from(step.clamp(4096, SLAB as i64)).unwrap_or(SLAB),
                ) else {
                    return false;
                };
                slab.extend_from_slice(&more);
            }
            let at_stream_end = base.packed + slab.len() as i64 >= l.pack_size;
            let index = index_from(&slab, base.unpacked);
            // The chunks that start before the end of the wanted bytes must be stored.
            if index
                .chunks
                .iter()
                .take_while(|c| (c.unpacked_offset as i64) < want_end)
                .any(|c| c.kind != zipfast::map::Lzma2ChunkKind::Stored)
            {
                return false;
            }
            let reached = index.end_position().unpacked as i64;
            if reached >= want_end {
                let mut filled = 0usize;
                for piece in index.plan(want_start as u64..want_end as u64) {
                    let zipfast::map::Lzma2Piece::Stored { packed, .. } = piece else {
                        return false;
                    };
                    let (from, to) = (packed.start as usize, packed.end as usize);
                    if to > slab.len() || filled + (to - from) > buf.len() {
                        return false;
                    }
                    buf[filled..filled + (to - from)].copy_from_slice(&slab[from..to]);
                    filled += to - from;
                }
                if filled != buf.len() {
                    return false;
                }
                // The last (stored, checked) chunk starting before the end of this read: the
                // next read starts there.
                if let Some(c) = index
                    .chunks
                    .iter()
                    .take_while(|c| (c.unpacked_offset as i64) < want_end)
                    .last()
                {
                    self.add_sync(SyncPoint {
                        unpacked: c.unpacked_offset as i64,
                        packed: base.packed + c.packed_offset as i64,
                    });
                }
                return true;
            }
            // Not there yet. Anything but the window's end stopped the walk: give up.
            let ended_by_window = matches!(
                index.end,
                zipfast::map::Lzma2End::Truncated { .. } | zipfast::map::Lzma2End::RangeEnd { .. }
            );
            // (How far a walk may go is bounded by the sync-point lookup above: MAX_WALK.)
            if !ended_by_window || at_stream_end {
                return false;
            }
            // What is left: the rest of the wanted output, its headers, one chunk that may run
            // past the end, minus the bytes of the unfinished chunk already read.
            let left = want_end - reached;
            let have_beyond = slab.len() as i64 - index.end_position().packed as i64;
            step = left + left / 16_384 + CHUNK_MAX + 64 - have_beyond.max(0);
            // Move the window past the complete chunks that end before the wanted bytes.
            if let Some(c) = index
                .chunks
                .iter()
                .take_while(|c| (c.unpacked().end as i64) <= want_start)
                .last()
            {
                let next = SyncPoint {
                    unpacked: c.unpacked().end as i64,
                    packed: base.packed + c.packed().end as i64,
                };
                let drop = (next.packed - base.packed) as usize;
                if drop > 0 && drop <= slab.len() {
                    slab.drain(..drop);
                    base = next;
                    if base.unpacked - last_noted >= 1024 * 1024 {
                        self.add_sync(base);
                        last_noted = base.unpacked;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Packs `data` as LZMA2 raw chunks with data-dependent chunk sizes, like 7-Zip does.
    fn pack_raw(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        let mut first = true;
        let mut seed = 0x9e37_79b9u32;
        while pos < data.len() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = (30_000 + (seed >> 16) as usize % 35_000)
                .min(data.len() - pos)
                .min(65_536);
            out.push(if first { 0x01 } else { 0x02 });
            out.push(((n - 1) >> 8) as u8);
            out.push(((n - 1) & 0xff) as u8);
            out.extend_from_slice(&data[pos..pos + n]);
            pos += n;
            first = false;
        }
        out.push(0);
        out
    }

    fn noise(len: usize, mut seed: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            v.push((seed >> 8) as u8);
        }
        v
    }

    fn layout_for(inner: &[u8], lead: usize, tail: usize, packed: &[u8]) -> Lzma2RawLayout {
        Lzma2RawLayout {
            inner_size: inner.len() as i64,
            inner_offset_in_folder: lead as i64,
            folder_unpack_size: (lead + inner.len() + tail) as i64,
            pack_size: packed.len() as i64,
        }
    }

    #[test]
    fn borrows_a_hole_from_raw_chunks() {
        let inner = noise(24 * 1024 * 1024, 7);
        let lead = noise(12_345, 9); // a small file before ours in the same solid folder
        let mut folder = lead.clone();
        folder.extend_from_slice(&inner);
        let packed = pack_raw(&folder);
        let layout = layout_for(&inner, lead.len(), 0, &packed);
        assert!(layout.looks_raw());

        let reads = RefCell::new(0i64);
        let src = Lzma2RawSource::new(
            layout,
            Box::new(|off, buf: &mut [u8]| {
                let o = off as usize;
                if o + buf.len() > packed.len() {
                    return false;
                }
                buf.copy_from_slice(&packed[o..o + buf.len()]);
                *reads.borrow_mut() += buf.len() as i64;
                true
            }),
        );

        // The target has everything but a 1.5 MB hole in the middle.
        let hole = (12_000_000i64, 1_500_000i64);
        let mut target = inner.clone();
        for b in &mut target[hole.0 as usize..(hole.0 + hole.1) as usize] {
            *b = 0;
        }
        let valid = |off: i64, len: i64| off + len <= hole.0 || off >= hole.0 + hole.1;
        let mut read_target = |off: i64, buf: &mut [u8]| {
            let o = off as usize;
            buf.copy_from_slice(&target[o..o + buf.len()]);
            true
        };

        // Without a sync point nothing can be read.
        let mut out = vec![0u8; hole.1 as usize];
        assert!(!src.read_inner(hole.0, &mut out));

        assert!(src.sync_before(hole.0, &mut read_target, &valid));
        assert!(src.read_inner(hole.0, &mut out));
        assert_eq!(out, &inner[hole.0 as usize..(hole.0 + hole.1) as usize]);

        // Probe windows either side of the hole read correctly too.
        let mut before = vec![0u8; 16 * 1024];
        assert!(src.read_inner(hole.0 - before.len() as i64, &mut before));
        assert_eq!(
            before,
            &inner[hole.0 as usize - before.len()..hole.0 as usize]
        );
        let mut after = vec![0u8; 16 * 1024];
        assert!(src.read_inner(hole.0 + hole.1, &mut after));
        assert_eq!(
            after,
            &inner[(hole.0 + hole.1) as usize..(hole.0 + hole.1) as usize + after.len()]
        );

        // Only the neighbourhood of the hole was fetched, not the 24 MB archive.
        assert!(*reads.borrow() < 5 * 1024 * 1024, "read {}", reads.borrow());
    }

    #[test]
    fn rejects_a_different_file_and_compressed_chunks() {
        let inner = noise(3 * 1024 * 1024, 21);
        let other = noise(3 * 1024 * 1024, 22);
        let packed = pack_raw(&other);
        let layout = layout_for(&other, 0, 0, &packed);
        let src = Lzma2RawSource::new(
            layout,
            Box::new(|off, buf: &mut [u8]| {
                let o = off as usize;
                if o + buf.len() > packed.len() {
                    return false;
                }
                buf.copy_from_slice(&packed[o..o + buf.len()]);
                true
            }),
        );
        let mut read_target = |off: i64, buf: &mut [u8]| {
            let o = off as usize;
            buf.copy_from_slice(&inner[o..o + buf.len()]);
            true
        };
        // A different file never yields a sync point.
        assert!(!src.sync_before(1_500_000, &mut read_target, &|_, _| true));

        // A compressed chunk in the way ends the walk.
        let same = pack_raw(&inner);
        let mut broken = same.clone();
        // Turn the chunk header that follows offset ~1.6 MB into an LZMA chunk header.
        let chains = find_chunk_chains(&broken[1_600_000..1_800_000]);
        let at = 1_600_000 + chains[0];
        broken[at] = 0x80;
        let layout = layout_for(&inner, 0, 0, &broken);
        let src = Lzma2RawSource::new(
            layout,
            Box::new(|off, buf: &mut [u8]| {
                let o = off as usize;
                if o + buf.len() > broken.len() {
                    return false;
                }
                buf.copy_from_slice(&broken[o..o + buf.len()]);
                true
            }),
        );
        assert!(src.sync_before(1_400_000, &mut read_target, &|_, _| true));
        let mut ok = vec![0u8; 50_000];
        assert!(src.read_inner(1_400_000, &mut ok));
        assert_eq!(ok, &inner[1_400_000..1_450_000]);
        let mut blocked = vec![0u8; 600_000];
        assert!(!src.read_inner(1_400_000, &mut blocked));
    }

    #[test]
    fn a_compressed_chunk_after_the_sync_point_ends_the_borrow() {
        // Stored chunks, one LZMA chunk (its packed bytes are not the output), stored again.
        // zipfast's index can walk across the LZMA chunk, but a borrow never goes past one:
        // reads from a sync point before it succeed only up to it.
        let folder = noise(4 * 1024 * 1024, 31);
        let lzma_at = 2_000_000usize;
        let lzma_len = 300_000usize;
        let mut packed = pack_raw(&folder[..lzma_at]);
        packed.pop(); // drop the end marker
        let u = lzma_len - 1;
        let p = 999usize;
        packed.extend_from_slice(&[
            0x80 | ((u >> 16) as u8),
            (u >> 8) as u8,
            u as u8,
            (p >> 8) as u8,
            p as u8,
        ]);
        packed.extend_from_slice(&noise(p + 1, 33));
        let mut rest = pack_raw(&folder[lzma_at + lzma_len..]);
        rest[0] = 0x02; // continues the stream: no dictionary reset
        packed.extend_from_slice(&rest);
        let layout = layout_for(&folder, 0, 0, &packed);
        let src = Lzma2RawSource::new(
            layout,
            Box::new(|off, buf: &mut [u8]| {
                let o = off as usize;
                if o + buf.len() > packed.len() {
                    return false;
                }
                buf.copy_from_slice(&packed[o..o + buf.len()]);
                true
            }),
        );
        let mut read_target = |off: i64, buf: &mut [u8]| {
            let o = off as usize;
            buf.copy_from_slice(&folder[o..o + buf.len()]);
            true
        };
        assert!(src.sync_before(1_900_000, &mut read_target, &|_, _| true));
        let mut before = vec![0u8; 50_000];
        assert!(src.read_inner(1_900_000, &mut before));
        assert_eq!(before, &folder[1_900_000..1_950_000]);
        let mut across = vec![0u8; 500_000];
        assert!(!src.read_inner(1_950_000, &mut across));
        let mut after = vec![0u8; 100_000];
        assert!(!src.read_inner(2_500_000, &mut after));
    }

    #[test]
    fn chain_finder_ignores_noise() {
        let data = noise(400_000, 3);
        // Random bytes almost never form three chained headers ending on a control byte.
        assert!(find_chunk_chains(&data).len() <= 2);
        let packed = pack_raw(&data);
        let chains = find_chunk_chains(&packed);
        assert_eq!(chains.first(), Some(&0));
    }

    #[test]
    fn looks_raw_follows_the_size_ratio() {
        let l = Lzma2RawLayout {
            inner_size: 100,
            inner_offset_in_folder: 0,
            folder_unpack_size: 1_000_000,
            pack_size: 1_000_100,
        };
        assert!(l.looks_raw());
        assert!(Lzma2RawLayout {
            pack_size: 947_000,
            ..l
        }
        .looks_raw());
        assert!(!Lzma2RawLayout {
            pack_size: 700_000,
            ..l
        }
        .looks_raw());
    }
}
