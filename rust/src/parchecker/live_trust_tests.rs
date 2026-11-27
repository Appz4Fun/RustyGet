//! Trusted slice verification (par-rs `set_trust_verified_slices`), decode-ahead
//! (`set_decode_ahead`), forged file descriptions, recovery volumes from
//! RAM (`on_recovery_volume_bytes`) and back-fill from held bytes (`backfill_from_bytes`).
use super::*;
use par_rs::create::{build_set, SetOptions, VolumeScheme};

const SLICE: usize = 4096;

fn bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 37) as u8
        })
        .collect()
}

struct Fixture {
    nzb_id: i32,
    dir: PathBuf,
    data: Vec<u8>,
    volumes: Vec<(String, Vec<u8>)>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        streaming_session_forget(self.nzb_id);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A one-file set (`part01.rar`, `slices` slices plus a short last one), its file on disk,
/// and a started live session sized for `slots` recovery slots.
fn fixture(nzb_id: i32, slices: usize, slots: usize, recovery: u32) -> Fixture {
    fixture_with(nzb_id, slices, slots, slots, recovery, VolumeScheme::Single)
}

/// [`fixture`] with `preallocated` of `slots` recovery slots allocated up front (as
/// `parlive::resources` sizes an unplanned session: a few preallocated, the cap and the
/// syndrome bytes for all of them) and the recovery blocks spread by `volumes`.
fn fixture_with(
    nzb_id: i32,
    slices: usize,
    preallocated: usize,
    slots: usize,
    recovery: u32,
    volumes: VolumeScheme,
) -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "rustyget-live-trust-{}-{nzb_id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A short last slice, so the trusted CRC must combine an unpadded tail.
    let data = bytes(slices * SLICE + 1000, nzb_id as u64);
    let built = build_set(
        "trust",
        &[("part01.rar", &data)],
        &SetOptions {
            slice_size: SLICE,
            recovery_blocks: recovery,
            first_exponent: 0,
            volumes,
            creator: "rustyget-test".to_string(),
            unicode_names: false,
            kernel: Kernel::detect(),
        },
    )
    .unwrap();
    let index = dir.join("trust.par2");
    std::fs::write(&index, &built.index).unwrap();
    std::fs::write(dir.join("part01.rar"), &data).unwrap();
    // A pool of its own (about 90 MB per session): sessions of these tests running at once
    // must not use up the shared one other tests take small reservations from.
    let pool = Arc::new(crate::parlive::BudgetPool::with_size(256 << 20));
    streaming_session_start_in_pool(
        &pool,
        nzb_id,
        &index,
        &dir,
        SessionOptions {
            preallocated_syndrome_slots: preallocated,
            max_syndrome_slots: slots,
            max_syndrome_bytes: (slots.max(1) * SLICE) as u64,
            max_working_bytes: 8 << 20,
            max_staging_bytes: 8 << 20,
            solve_mem_limit_bytes: 16 << 20,
            defer_backfill: true,
            ..SessionOptions::default()
        },
    )
    .unwrap();
    Fixture {
        nzb_id,
        dir,
        data,
        volumes: built.volumes,
    }
}

fn file_id(nzb_id: i32) -> FileId {
    active_streaming_session(nzb_id)
        .unwrap()
        .session
        .file_id_by_name(b"part01.rar")
        .unwrap()
}

/// Every slice handed over from RAM, as the article cache's capture does.
fn feed_all(f: &Fixture) {
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let id = file_id(f.nzb_id);
    for (i, chunk) in f.data.chunks(SLICE).enumerate() {
        entry
            .session
            .on_article_finished_memory_only(
                &id,
                (i * SLICE) as u64,
                chunk,
                par_rs::crc::crc32(chunk),
            )
            .unwrap();
    }
}

fn target_reads(nzb_id: i32) -> u64 {
    active_streaming_session(nzb_id)
        .unwrap()
        .session
        .io_stats()
        .target_verification_bytes_read
}

fn check(nzb_id: i32) -> (bool, Option<par_rs::hasher::FileHashes>) {
    let entry = active_streaming_session(nzb_id).unwrap();
    let id = file_id(nzb_id);
    let (report, hashes) = entry
        .session
        .repair_available_targets_with_hashes_now(&[id])
        .unwrap();
    let ok = report
        .files
        .iter()
        .any(|(fid, status, ok)| *fid == id && *ok && *status == VolumeStatus::CompleteAndVerified);
    (
        ok,
        hashes
            .into_iter()
            .find(|(fid, _)| *fid == id)
            .map(|(_, h)| h),
    )
}

/// Live sessions start with trusted slices on: a target whose every slice verified gets
/// the same hash receipt as the whole-file read, without reading the file.
#[test]
fn trusted_slices_give_the_full_read_hashes_without_reading_the_target() {
    let f = fixture(0x7a75_0001, 6, 2, 2);
    assert_eq!(streaming_session_trusts_slices(f.nzb_id), Some(true));
    feed_all(&f);
    let (ok, trusted) = check(f.nzb_id);
    assert!(ok);
    let trusted = trusted.expect("a hash receipt for the verified target");
    assert_eq!(
        target_reads(f.nzb_id),
        0,
        "no byte of the target read again"
    );
    assert_eq!(trusted.len, f.data.len() as u64);
    assert_eq!(trusted.crc32, par_rs::crc::crc32(&f.data));

    // The same check with the whole-file read put back.
    streaming_session_distrust(f.nzb_id, "test");
    assert_eq!(streaming_session_trusts_slices(f.nzb_id), Some(false));
    let (ok, full) = check(f.nzb_id);
    assert!(ok);
    assert_eq!(
        full.unwrap(),
        trusted,
        "trusted receipt equals the full read"
    );
    assert_eq!(target_reads(f.nzb_id), f.data.len() as u64);
}

/// A write into a verified range after verification is what trusted slices cannot see on
/// their own. Reported with `on_range_replaced` (as every RustyGet writer path does, or
/// it stops live PAR), par-rs re-reads the slice, finds it diverged, and the target is not
/// vouched for; the full check decides and fails it.
#[test]
fn an_overwrite_after_verification_is_caught_because_it_is_reported() {
    let f = fixture(0x7a75_0002, 6, 2, 2);
    feed_all(&f);
    let path = f.dir.join("part01.rar");
    let mut disk = f.data.clone();
    disk[2 * SLICE + 10..2 * SLICE + 20].fill(0xEE);
    std::fs::write(&path, &disk).unwrap();
    let reconciled = streaming_session_on_range_replaced_by_name(
        f.nzb_id,
        b"part01.rar",
        (2 * SLICE) as u64,
        SLICE as u64,
    );
    assert_eq!(
        reconciled,
        Some(false),
        "the diverged slice is not reconciled"
    );
    let entry = active_streaming_session(f.nzb_id).unwrap();
    assert!(!entry.session.diverged_slices().is_empty());
    let (ok, hashes) = check(f.nzb_id);
    assert!(!ok, "the overwritten target fails");
    assert!(hashes.is_none(), "no receipt for it");
}

/// The control for the test above: the same overwrite, not reported, passes a trusted
/// check. This is why every write into a par target must be reported (or turn trust off).
#[test]
fn an_unreported_overwrite_would_pass_a_trusted_check() {
    let f = fixture(0x7a75_0003, 6, 2, 2);
    feed_all(&f);
    let mut disk = f.data.clone();
    disk[2 * SLICE + 10..2 * SLICE + 20].fill(0xEE);
    std::fs::write(f.dir.join("part01.rar"), &disk).unwrap();
    let (ok, _) = check(f.nzb_id);
    assert!(ok, "trusted slices do not see an unreported write");
    // With trust revoked (what a RustyGet path that cannot report does) the full read
    // catches it.
    streaming_session_distrust(f.nzb_id, "unreported write");
    let (ok, hashes) = check(f.nzb_id);
    assert!(!ok);
    assert!(hashes.is_none());
}

/// A slice that diverged (its bytes on disk differ from what verified) and was then put
/// right on disk and reported again verifies from disk: the target passes once more.
#[test]
fn a_diverged_slice_put_right_and_reported_passes_again() {
    let f = fixture(0x7a75_0004, 6, 2, 2);
    feed_all(&f);
    let path = f.dir.join("part01.rar");
    let mut disk = f.data.clone();
    disk[SLICE..SLICE + 4].fill(0);
    std::fs::write(&path, &disk).unwrap();
    assert_eq!(
        streaming_session_on_range_replaced_by_name(f.nzb_id, b"part01.rar", SLICE as u64, 4),
        Some(false)
    );
    std::fs::write(&path, &f.data).unwrap();
    // Reported again: par-rs re-reads the slice from disk, and it matches IFSC once more.
    let _ = streaming_session_on_range_replaced_by_name(f.nzb_id, b"part01.rar", SLICE as u64, 4);
    let entry = active_streaming_session(f.nzb_id).unwrap();
    assert!(entry.session.diverged_slices().is_empty());
    let (ok, hashes) = check(f.nzb_id);
    assert!(ok);
    assert_eq!(hashes.unwrap().crc32, par_rs::crc::crc32(&f.data));
}

/// A target the trusted shortcut cannot vouch for (here: bytes past its described length)
/// falls back to the whole-file check, which reads it and fails it.
#[test]
fn a_target_trust_cannot_vouch_for_falls_back_to_the_full_check() {
    let f = fixture(0x7a75_000a, 6, 2, 2);
    feed_all(&f);
    let mut disk = f.data.clone();
    disk.push(0);
    std::fs::write(f.dir.join("part01.rar"), &disk).unwrap();
    let (ok, hashes) = check(f.nzb_id);
    assert!(!ok);
    assert!(hashes.is_none());
    assert!(target_reads(f.nzb_id) > 0, "the full check read the target");
}

/// A recovery volume whose every byte went through the recovery tee is absorbed from RAM:
/// no byte of it read back, and the same blocks become ready as by reading it.
#[test]
fn a_teed_recovery_volume_is_absorbed_without_reading_it_back() {
    let f = fixture(0x7a75_0005, 6, 4, 3);
    let read_before = active_streaming_session(f.nzb_id)
        .unwrap()
        .session
        .io_stats()
        .recovery_bytes_read;
    let (name, volume) = &f.volumes[0];
    let path = f.dir.join(name);
    std::fs::write(&path, volume).unwrap();
    // Written in three pieces, out of order, as the cache flush and the writer hand them.
    let third = volume.len() / 3;
    let pieces = [
        (third, &volume[third..2 * third]),
        (0, &volume[..third]),
        (2 * third, &volume[2 * third..]),
    ];
    for (offset, piece) in pieces {
        streaming_session_recovery_bytes_written(
            f.nzb_id,
            77,
            volume.len() as u64,
            &[(offset as u64, piece)],
        );
    }
    let count = streaming_session_on_recovery_volume_completed(f.nzb_id, &path, Some(77), true)
        .unwrap()
        .unwrap();
    assert_eq!(count, 3);
    let entry = active_streaming_session(f.nzb_id).unwrap();
    assert_eq!(
        entry.session.io_stats().recovery_bytes_read,
        read_before,
        "no byte of the volume read back"
    );
    assert_eq!(
        entry
            .metrics
            .recovery_volumes_from_ram
            .load(Ordering::Relaxed),
        1
    );
    assert_eq!(entry.session.status().ready_recovery, 3);
    // The charge is returned.
    assert_eq!(entry.articles.lock().unwrap().bytes, 0);
}

/// A volume that did not go through the tee whole (a piece missing) is read back as before.
#[test]
fn a_partly_teed_recovery_volume_is_read_back() {
    let f = fixture(0x7a75_0006, 6, 4, 3);
    let read_before = active_streaming_session(f.nzb_id)
        .unwrap()
        .session
        .io_stats()
        .recovery_bytes_read;
    let (name, volume) = &f.volumes[0];
    let path = f.dir.join(name);
    std::fs::write(&path, volume).unwrap();
    streaming_session_recovery_bytes_written(
        f.nzb_id,
        78,
        volume.len() as u64,
        &[(0, &volume[..volume.len() / 2])],
    );
    let count = streaming_session_on_recovery_volume_completed(f.nzb_id, &path, Some(78), true)
        .unwrap()
        .unwrap();
    assert_eq!(count, 3);
    let entry = active_streaming_session(f.nzb_id).unwrap();
    assert!(
        entry.session.io_stats().recovery_bytes_read >= read_before + volume.len() as u64,
        "the volume is read back"
    );
    assert_eq!(
        entry
            .metrics
            .recovery_volumes_from_ram
            .load(Ordering::Relaxed),
        0
    );
    assert_eq!(entry.articles.lock().unwrap().bytes, 0);
}

/// Recovery slots added after slices verified are filled from bytes RustyGet still holds
/// (the cache flush, completion assembly): the back-fill then reads nothing from disk.
#[test]
fn slots_added_later_are_back_filled_from_held_bytes() {
    let f = fixture(0x7a75_0007, 6, 1, 4);
    let entry = active_streaming_session(f.nzb_id).unwrap();
    feed_all(&f);
    let before = entry.session.io_stats().backfill_bytes_read;
    // Two more slots after every slice verified.
    entry.session.set_max_syndrome_slots(3).unwrap();
    let (name, volume) = &f.volumes[0];
    let path = f.dir.join(name);
    std::fs::write(&path, volume).unwrap();
    entry
        .session
        .on_recovery_volume_arrived(&path, None)
        .unwrap();
    assert!(entry.session.needs_backfill());
    // The held bytes arrive as written articles, smaller than a slice.
    let id = file_id(f.nzb_id);
    let items: Vec<(u64, &[u8])> = f
        .data
        .chunks(1500)
        .enumerate()
        .map(|(i, c)| ((i * 1500) as u64, c))
        .collect();
    // The flush only keeps the bytes (it holds the cache lock; a fold could wait on a repair).
    let kept = entry.backfill_held(&id, &items);
    assert_eq!(kept, f.data.len() as u64, "one contiguous run kept");
    assert_eq!(entry.metrics.ram_backfilled_slices.load(Ordering::Relaxed), 0);
    // The back-fill thread folds them first, then reads only what nobody supplied.
    let filled = entry.apply_held_backfill();
    assert_eq!(filled, 7, "every slice, the short last one too, filled from RAM");
    assert_eq!(entry.held_backfill.lock().unwrap().1, 0, "the kept bytes are released");
    entry.session.backfill_pending().unwrap();
    let reread = entry.session.io_stats().backfill_bytes_read - before;
    assert_eq!(reread, 0, "nothing read from disk for the new slots");
    assert_eq!(
        entry.metrics.ram_backfilled_slices.load(Ordering::Relaxed),
        7
    );
}

/// M6f with RustyGet's ordering: an article handed over as committed (its bytes in the
/// preallocated target, gone from the cache) and a RAM-only article completing the same
/// slice verify the slice with no transient damage and no unpause. RustyGet hands an
/// article over as committed only after its write, so par-rs never reads the preallocated
/// zero gap.
#[test]
fn a_ram_article_completing_a_committed_slice_raises_no_transient_damage() {
    let f = fixture(0x7a75_0008, 6, 2, 2);
    let path = f.dir.join("part01.rar");
    // Preallocated as DirectWrite does: zeros until each article is written.
    let mut disk = vec![0u8; f.data.len()];
    std::fs::write(&path, &disk).unwrap();
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let half = SLICE / 2;
    let identity = (None, &b"part01.rar"[..], &b""[..]);
    for slice in 0..2usize {
        let first = slice * SLICE;
        // The first half is written, then captured with its cache bytes gone (committed).
        disk[first..first + half].copy_from_slice(&f.data[first..first + half]);
        std::fs::write(&path, &disk).unwrap();
        let a = &f.data[first..first + half];
        streaming_session_defer_committed_article(
            f.nzb_id,
            identity,
            first as u64,
            half as u64,
            par_rs::crc::crc32(a),
            Some(&path),
            || None,
        )
        .unwrap()
        .unwrap();
        // The second half arrives from RAM (not on disk yet) and completes the slice.
        let b = f.data[first + half..first + SLICE].to_vec();
        let crc = par_rs::crc::crc32(&b);
        streaming_session_defer_committed_article(
            f.nzb_id,
            identity,
            (first + half) as u64,
            half as u64,
            crc,
            Some(&path),
            move || Some(b),
        )
        .unwrap()
        .unwrap();
        streaming_session_drain_articles(f.nzb_id).unwrap().unwrap();
        let status = entry.session.status();
        assert_eq!(status.damaged, 0, "no transient damage on slice {slice}");
        assert_eq!(status.verified, slice + 1);
        assert!(
            entry.pending_unpause_ids.lock().unwrap().is_empty(),
            "no recovery volume asked for"
        );
    }
    // Only the committed halves were read, once each.
    assert_eq!(
        entry.session.io_stats().source_bytes_read,
        (2 * half) as u64
    );
}

/// The hazard M6f's caution names, at the par-rs boundary: a committed range reported
/// before its write reads the preallocated zeros. Since par-rs 2524277 (#145) that slice is
/// not damaged: the zeros do not match the CRC the article was reported with, so the slice
/// waits, and once the bytes land the next `complete_staged_slices` (RustyGet runs it after
/// each flush, `staged_due`) verifies it. RustyGet's capture never reports a committed
/// article before its write (test above); this is the safety net for a write-back cache.
#[test]
fn a_committed_range_reported_before_its_write_waits_for_it() {
    let f = fixture(0x7a75_0009, 6, 2, 2);
    let path = f.dir.join("part01.rar");
    std::fs::write(&path, vec![0u8; f.data.len()]).unwrap();
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let id = file_id(f.nzb_id);
    let half = SLICE / 2;
    let a = &f.data[..half];
    entry
        .session
        .on_article_finished(&id, 0, half as u64, par_rs::crc::crc32(a), None)
        .unwrap();
    let b = &f.data[half..SLICE];
    let outcome = entry
        .session
        .on_article_finished_memory_only(&id, half as u64, b, par_rs::crc::crc32(b))
        .unwrap();
    assert!(
        outcome
            .events
            .iter()
            .all(|e| e.state != SliceState::Damaged),
        "the zero gap does not match its article CRC: the slice waits"
    );
    // The write lands; the staged check after the flush verifies the slice.
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[..half].copy_from_slice(a);
    std::fs::write(&path, &bytes).unwrap();
    let done = entry.session.complete_staged_slices().unwrap();
    assert!(
        done.events
            .iter()
            .any(|e| e.slice_idx == 0 && e.state == SliceState::VerifiedGood),
        "{:?}",
        done.events
    );
}

/// The recovery tee accepts the same article twice but voids on any other overlap, so its
/// coverage count never claims bytes that were not written; a void or forgotten tee gives
/// its charge back.
#[test]
fn recovery_tee_voids_on_overlap_and_returns_its_charge() {
    let f = fixture(0x7a75_000b, 6, 4, 3);
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let volume = vec![7u8; 300];
    streaming_session_recovery_bytes_written(f.nzb_id, 90, 300, &[(0, &volume[..150])]);
    streaming_session_recovery_bytes_written(f.nzb_id, 90, 300, &[(0, &volume[..150])]);
    assert_eq!(entry.articles.lock().unwrap().bytes, 300, "charged once, whole volume");
    // [100, 250) overlaps [0, 150): with it `covered` would reach 300 with [250, 300) unset.
    streaming_session_recovery_bytes_written(f.nzb_id, 90, 300, &[(100, &volume[100..250])]);
    assert_eq!(entry.articles.lock().unwrap().bytes, 0, "voided: charge returned");
    assert!(entry.take_recovery_bytes(90, 300).is_none());
    // A lost volume's tee goes with its charge.
    streaming_session_recovery_bytes_written(f.nzb_id, 91, 300, &[(0, &volume[..100])]);
    assert_eq!(entry.articles.lock().unwrap().bytes, 300);
    streaming_session_on_recovery_volume_lost(f.nzb_id, 91);
    assert_eq!(entry.articles.lock().unwrap().bytes, 0);
    assert!(entry.recovery_tees.lock().unwrap().get(&91).is_none());
}

/// All recovery tees together stay within half the snapshot limit. A tee that would pass it
/// first lends free ParBuffer to raise the limit (a volume not teed is read back whole).
#[test]
fn recovery_tees_together_stay_within_half_the_snapshot_limit() {
    let f = fixture(0x7a75_000c, 6, 4, 3);
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let limit = entry.article_byte_limit.load(Ordering::Acquire);
    let size = limit / 3;
    let piece = vec![1u8; 16];
    streaming_session_recovery_bytes_written(f.nzb_id, 1, size, &[(0, &piece)]);
    // No loan left: the half rule holds.
    let lent = entry.lent_memory.swap(u64::MAX / 4, Ordering::AcqRel);
    streaming_session_recovery_bytes_written(f.nzb_id, 2, size, &[(0, &piece)]);
    {
        let tees = entry.recovery_tees.lock().unwrap();
        assert!(!tees[&1].void, "the first fits");
        assert!(tees[&2].void, "the second would pass half the limit");
    }
    assert_eq!(entry.articles.lock().unwrap().bytes, size);
    // With free ParBuffer to lend, the limit grows to take the third.
    entry.lent_memory.store(lent, Ordering::Release);
    streaming_session_recovery_bytes_written(f.nzb_id, 3, size, &[(0, &piece)]);
    assert!(!entry.recovery_tees.lock().unwrap()[&3].void, "a loan makes room");
    assert!(entry.article_byte_limit.load(Ordering::Acquire) >= 4 * size);
    assert_eq!(entry.articles.lock().unwrap().bytes, 2 * size);
}

/// The recovery volume arrives first; slice `lost` fails for good, every other slice arrives
/// in order through the entry points the download uses. Returns the index of the slice whose
/// hand-over brought the rebuilt slice on (`None` when nothing was rebuilt).
fn feed_with_loss(f: &Fixture, lost: usize) -> Option<usize> {
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let (name, volume) = &f.volumes[0];
    let path = f.dir.join(name);
    std::fs::write(&path, volume).unwrap();
    entry.session.on_recovery_volume_arrived(&path, None).unwrap();
    let id = file_id(f.nzb_id);
    let mut handed_at = None;
    for (i, chunk) in f.data.chunks(SLICE).enumerate() {
        let offset = (i * SLICE) as u64;
        if i == lost {
            streaming_session_on_article_failed_permanently(
                f.nzb_id,
                &id,
                offset,
                chunk.len() as u64,
            )
            .unwrap()
            .unwrap();
        } else {
            streaming_session_on_article_finished_by_name(
                f.nzb_id,
                b"part01.rar",
                offset,
                chunk.len() as u64,
                par_rs::crc::crc32(chunk),
                Some(chunk),
            )
            .unwrap()
            .unwrap();
        }
        if handed_at.is_none() && entry.metrics.rebuilt_ahead_slices.load(Ordering::Relaxed) > 0 {
            handed_at = Some(i);
        }
    }
    handed_at
}

/// Decode-ahead (par-rs #126/#127): a slice lost early is rebuilt, written into its target
/// and handed on within the call that verifies the last other slice, before any repair call,
/// so the download's end finds nothing left to repair and the target is whole.
#[test]
fn decode_ahead_rebuilds_a_lost_slice_before_the_download_ends() {
    let f = fixture(0x7a75_000d, 16, 2, 2);
    let path = f.dir.join("part01.rar");
    // The lost article's bytes never reach the file.
    let mut disk = f.data.clone();
    disk[SLICE..2 * SLICE].fill(0);
    std::fs::write(&path, &disk).unwrap();
    let handed_at = feed_with_loss(&f, 1);
    let last = f.data.len().div_ceil(SLICE) - 1;
    assert_eq!(handed_at, Some(last), "rebuilt with the last article, not at a repair call");
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let status = entry.session.status();
    assert_eq!(status.damaged, 0, "the lost slice is verified now");
    assert_eq!(status.verified, status.total);
    assert_eq!(std::fs::read(&path).unwrap(), f.data, "written into the target");
    assert_eq!(entry.metrics.rebuilt_ahead_slices.load(Ordering::Relaxed), 1);
    // Verified, but it lost an article: the repair as the download ends still takes the
    // file (nothing left to rebuild) and certifies it, so no batch par-check runs.
    assert_eq!(
        streaming_session_download_end_targets(f.nzb_id),
        Some(vec![b"part01.rar".to_vec()])
    );
    // The file is still being written: its rebuilt slice waits for a second look.
    assert_eq!(entry.rebuilt_pending(), 1);
    let id = file_id(f.nzb_id);
    with_live_entry(f.nzb_id, &entry, |e| e.recheck_rebuilt(f.nzb_id, id)).unwrap();
    assert_eq!(entry.rebuilt_pending(), 0);
    assert_eq!(entry.metrics.rebuilt_rechecked.load(Ordering::Relaxed), 1);
    assert!(entry.session.diverged_slices().is_empty());
    let (ok, hashes) = check(f.nzb_id);
    assert!(ok);
    assert_eq!(hashes.unwrap().crc32, par_rs::crc::crc32(&f.data));
    assert!(entry
        .io_stats_line(f.nzb_id)
        .contains("1 slice(s) rebuilt ahead (1 re-verified at completion), "));
}

/// The write order of decode-ahead: a late write of a corrupt copy into a slice rebuilt
/// while its file was still being written undoes the rebuild. The second look at the file's
/// completion finds the slice diverged, so the trusted check fails the target (and the set
/// is repaired from disk) instead of passing it.
#[test]
fn a_corrupt_write_after_the_rebuild_is_caught_at_completion() {
    let f = fixture(0x7a75_000e, 16, 2, 2);
    let path = f.dir.join("part01.rar");
    let mut disk = f.data.clone();
    disk[3 * SLICE..4 * SLICE].fill(0);
    std::fs::write(&path, &disk).unwrap();
    assert!(feed_with_loss(&f, 3).is_some());
    assert_eq!(std::fs::read(&path).unwrap(), f.data);
    // A retry's CRC-failed copy lands after the rebuild.
    let mut late = f.data.clone();
    late[3 * SLICE + 100..3 * SLICE + 200].fill(0x5A);
    std::fs::write(&path, &late).unwrap();
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let id = file_id(f.nzb_id);
    with_live_entry(f.nzb_id, &entry, |e| e.recheck_rebuilt(f.nzb_id, id)).unwrap();
    assert!(!entry.session.diverged_slices().is_empty());
    let (ok, hashes) = check(f.nzb_id);
    assert!(!ok, "the overwritten target fails");
    assert!(hashes.is_none());
}

/// A slice rebuilt in a file whose download already completed is final: nothing is read
/// back for it.
#[test]
fn a_slice_rebuilt_in_a_completed_file_is_not_read_again() {
    let f = fixture(0x7a75_000f, 16, 2, 2);
    let path = f.dir.join("part01.rar");
    let mut disk = f.data.clone();
    disk[2 * SLICE..3 * SLICE].fill(0);
    std::fs::write(&path, &disk).unwrap();
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let id = file_id(f.nzb_id);
    entry.completed_sources.lock().unwrap().insert(id);
    assert!(feed_with_loss(&f, 2).is_some());
    assert_eq!(entry.rebuilt_pending(), 0);
    let reads = target_reads(f.nzb_id);
    with_live_entry(f.nzb_id, &entry, |e| e.recheck_rebuilt(f.nzb_id, id)).unwrap();
    assert_eq!(target_reads(f.nzb_id), reads);
    assert_eq!(entry.metrics.rebuilt_rechecked.load(Ordering::Relaxed), 0);
    assert_eq!(std::fs::read(&path).unwrap(), f.data);
}

/// Rebuilt bytes reach live extraction at once: the volume the download is writing (its
/// direct-write output is the file par-rs wrote into) counts the rebuilt slice as written,
/// so an extraction parked on the lost range reads on.
#[test]
fn rebuilt_slices_are_handed_to_live_extraction() {
    let f = fixture(0x7a75_0010, 16, 2, 2);
    let path = f.dir.join("part01.rar");
    let mut disk = f.data.clone();
    disk[5 * SLICE..6 * SLICE].fill(0);
    std::fs::write(&path, &disk).unwrap();
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let id = file_id(f.nzb_id);
    let queue_id = 41;
    entry.queue_bindings.lock().unwrap().insert(queue_id, id);
    entry.note_written(id, &path);
    {
        let _operation = entry.operation.lock().unwrap();
        entry.apply_target_paths().unwrap();
    }
    let out = path.as_os_str().as_encoded_bytes().to_vec();
    crate::livewrite::register(f.nzb_id, queue_id, &out, &out, f.data.len() as u64, false);
    let volume = crate::livewrite::lookup(f.nzb_id, &out).unwrap();
    // Everything but the lost slice is written by the download.
    let len = f.data.len() as u64;
    crate::livewrite::written(f.nzb_id, queue_id, 0, (5 * SLICE) as u64);
    crate::livewrite::written(f.nzb_id, queue_id, (6 * SLICE) as u64, len - (6 * SLICE) as u64);
    assert_eq!(volume.written_end((5 * SLICE) as u64), None);
    assert!(feed_with_loss(&f, 5).is_some());
    assert_eq!(volume.written_end(0), Some(len), "one written run now");
    crate::livewrite::forget(f.nzb_id);
}

/// par-rs #133: a FileDesc packet edited to an implausible length (its checksum fixed, so it
/// loads) made finalize allocate gigabytes. RustyGet treats the set as unusable and carries
/// on: the batch repairer reports insufficient critical data with the reason, and live PAR
/// does not start, without allocating for the claimed slices.
#[test]
fn a_forged_file_description_is_an_unusable_par_set() {
    use par_rs::packet::{encode_packet, SetId, TYPE_FILEDESC};
    let dir = std::env::temp_dir().join(format!(
        "rustyget-forged-filedesc-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let data = bytes(8 * SLICE, 0x0f0f);
    let built = build_set(
        "forged",
        &[("part01.rar", &data)],
        &SetOptions {
            slice_size: SLICE,
            recovery_blocks: 2,
            first_exponent: 0,
            volumes: VolumeScheme::Single,
            creator: "rustyget-test".to_string(),
            unicode_names: false,
            kernel: Kernel::detect(),
        },
    )
    .unwrap();
    // Every FileDesc packet claims 2^40 slices, re-encoded so its MD5 matches again.
    let mut forged = Vec::new();
    let mut at = 0;
    let index = &built.index;
    while at + 64 <= index.len() {
        let len = u64::from_le_bytes(index[at + 8..at + 16].try_into().unwrap()) as usize;
        let packet = &index[at..at + len];
        if packet[48..64] == TYPE_FILEDESC {
            let set_id = SetId(packet[32..48].try_into().unwrap());
            let mut body = packet[64..].to_vec();
            body[48..56].copy_from_slice(&((SLICE as u64) << 40).to_le_bytes());
            forged.extend(encode_packet(&set_id, &TYPE_FILEDESC, &body));
        } else {
            forged.extend_from_slice(packet);
        }
        at += len;
    }
    assert_ne!(forged, built.index);
    let par = dir.join("forged.par2");
    std::fs::write(&par, &forged).unwrap();
    std::fs::write(dir.join("part01.rar"), &data).unwrap();

    let mut repairer = RustParRepairer::new(dir.clone(), 1, 64, false, false, Vec::new());
    assert!(matches!(
        repairer.preprocess(&par, None),
        ParResult::InsufficientCriticalData
    ));
    let reason = repairer.load_error.clone().unwrap_or_default();
    assert!(reason.contains("source slices"), "{reason}");

    let nzb_id = 0x7a75_0011;
    let started = streaming_session_start(
        nzb_id,
        &par,
        &dir,
        SessionOptions {
            preallocated_syndrome_slots: 2,
            max_syndrome_slots: 2,
            max_syndrome_bytes: (2 * SLICE) as u64,
            max_working_bytes: 8 << 20,
            max_staging_bytes: 8 << 20,
            solve_mem_limit_bytes: 16 << 20,
            defer_backfill: true,
            ..SessionOptions::default()
        },
    );
    assert!(started.is_err(), "no live session for a forged set");
    assert!(active_streaming_session(nzb_id).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A rebuilt slice that cannot be re-verified at its file's completion (its target is gone)
/// is not left trusted: the whole-file check is put back for the repair at download end.
#[test]
fn a_rebuilt_slice_that_cannot_be_rechecked_puts_the_full_check_back() {
    let f = fixture(0x7a75_0012, 16, 2, 2);
    let path = f.dir.join("part01.rar");
    let mut disk = f.data.clone();
    disk[4 * SLICE..5 * SLICE].fill(0);
    std::fs::write(&path, &disk).unwrap();
    assert!(feed_with_loss(&f, 4).is_some());
    assert_eq!(streaming_session_trusts_slices(f.nzb_id), Some(true));
    std::fs::remove_file(&path).unwrap();
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let id = file_id(f.nzb_id);
    assert!(with_live_entry(f.nzb_id, &entry, |e| e.recheck_rebuilt(f.nzb_id, id)).is_err());
    assert_eq!(streaming_session_trusts_slices(f.nzb_id), Some(false));
}

/// The end repair of a file with a slice rebuilt ahead certifies it once: afterwards the file
/// is no end-repair target any more, so the repair does not run again (and hold
/// post-processing back) while the download waits for it.
#[test]
fn a_file_rebuilt_ahead_is_certified_once_by_the_end_repair() {
    let f = fixture(0x7a75_0013, 16, 2, 2);
    let path = f.dir.join("part01.rar");
    let mut disk = f.data.clone();
    disk[6 * SLICE..7 * SLICE].fill(0);
    std::fs::write(&path, &disk).unwrap();
    assert!(feed_with_loss(&f, 6).is_some());
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let id = file_id(f.nzb_id);
    with_live_entry(f.nzb_id, &entry, |e| e.recheck_rebuilt(f.nzb_id, id)).unwrap();
    assert_eq!(
        streaming_session_download_end_targets(f.nzb_id),
        Some(vec![b"part01.rar".to_vec()])
    );
    let request = streaming_session_prepare_repair(
        f.nzb_id,
        vec![crate::parlive::WriterTarget {
            file_id: 42,
            path: path.clone(),
        }],
        crate::parlive::reserve_control(f.nzb_id, 4096, 500 << 20).unwrap(),
    )
    .unwrap();
    let lease = request.acquire().unwrap();
    let (report, released) = lease
        .repair_and_publish(|verified| verified.len(), || ())
        .unwrap();
    assert!(report.repaired_slices.is_empty(), "nothing left to rebuild");
    assert_eq!(released, 1, "certified");
    assert_eq!(streaming_session_download_end_targets(f.nzb_id), None);
    assert_eq!(std::fs::read(&path).unwrap(), f.data);
}

/// Feeds the set with slices `lost` failed for good (recovery first), repairs and publishes the
/// target through the end-repair path, and returns the session's I/O (par-rs `io_stats`).
fn repair_with_loss(f: &Fixture, lost: &[usize], decode_ahead: bool) -> LiveIoStats {
    let entry = active_streaming_session(f.nzb_id).unwrap();
    entry.session.set_decode_ahead(decode_ahead);
    // The index itself was read when the session started; nothing after it may be read.
    let index_read = entry.session.io_stats().recovery_bytes_read;
    let path = f.dir.join("part01.rar");
    let mut disk = f.data.clone();
    for &s in lost {
        let end = ((s + 1) * SLICE).min(disk.len());
        disk[s * SLICE..end].fill(0);
    }
    std::fs::write(&path, &disk).unwrap();
    let (name, volume) = &f.volumes[0];
    let vpath = f.dir.join(name);
    std::fs::write(&vpath, volume).unwrap();
    streaming_session_recovery_bytes_written(f.nzb_id, 77, volume.len() as u64, &[(0, volume)]);
    streaming_session_on_recovery_volume_completed(f.nzb_id, &vpath, Some(77), true)
        .unwrap()
        .unwrap();
    let id = file_id(f.nzb_id);
    for (i, chunk) in f.data.chunks(SLICE).enumerate() {
        let offset = (i * SLICE) as u64;
        if lost.contains(&i) {
            streaming_session_on_article_failed_permanently(f.nzb_id, &id, offset, chunk.len() as u64)
                .unwrap()
                .unwrap();
        } else {
            // As the download hands them over (through `record`, which takes rebuilt slices).
            streaming_session_on_article_finished_by_name(
                f.nzb_id,
                b"part01.rar",
                offset,
                chunk.len() as u64,
                par_rs::crc::crc32(chunk),
                Some(chunk),
            )
            .unwrap()
            .unwrap();
        }
    }
    // The file completes: slices rebuilt ahead while it was being written are looked at again.
    with_live_entry(f.nzb_id, &entry, |e| e.recheck_rebuilt(f.nzb_id, id)).unwrap();
    assert_eq!(
        streaming_session_download_end_targets(f.nzb_id),
        Some(vec![b"part01.rar".to_vec()])
    );
    let request = streaming_session_prepare_repair(
        f.nzb_id,
        vec![crate::parlive::WriterTarget {
            file_id: 42,
            path: path.clone(),
        }],
        crate::parlive::reserve_control(f.nzb_id, 4096, 500 << 20).unwrap(),
    )
    .unwrap();
    let (report, released) = request
        .acquire()
        .unwrap()
        .repair_and_publish(|verified| verified.len(), || ())
        .unwrap();
    assert_eq!(released, 1, "certified");
    let rebuilt = entry.metrics.rebuilt_ahead_slices.load(Ordering::Relaxed) as usize;
    assert_eq!(report.repaired_slices.len() + rebuilt, lost.len());
    assert_eq!(rebuilt > 0, decode_ahead);
    assert_eq!(std::fs::read(&path).unwrap(), f.data, "byte-exact");
    // RustyGet's own catch-up reads included.
    let mut io = streaming_session_io_stats(f.nzb_id).unwrap();
    io.recovery_bytes_read -= index_read;
    io
}

/// Zero re-reads (par-rs #143): with every article from RAM, the recovery volume from its
/// written bytes, trusted slices and the slots planned up front, a live repair whose call
/// rebuilds the lost slices reads nothing back: no source byte, no back-fill, and no rebuilt
/// slice re-read for the refold or the whole-file check (`target_verification_bytes_read`,
/// which counts the refold read-back since #143).
///
/// With decode-ahead the slices are rebuilt within the last article's hand-over, while their
/// file is still being written; the only read is the deliberate second look at exactly those
/// slices when the file completes (`recheck_rebuilt`: a late write could have undone them).
/// Everything else is read nowhere.
#[test]
fn a_live_repair_reads_nothing_back() {
    let lost = [2, 9, 14];
    for (n, decode_ahead) in [(0x7a75_0014, false), (0x7a75_0015, true)] {
        let f = fixture(n, 16, 3, 3);
        let io = repair_with_loss(&f, &lost, decode_ahead);
        let rechecked = if decode_ahead { (lost.len() * SLICE) as u64 } else { 0 };
        assert_eq!(
            (
                io.source_bytes_read,
                io.backfill_bytes_read,
                io.recovery_bytes_read,
                io.target_verification_bytes_read,
            ),
            (rechecked, 0, 0, 0),
            "decode_ahead={decode_ahead}: {io:?}"
        );
        let entry = active_streaming_session(n).unwrap();
        assert_eq!(
            entry.metrics.rebuilt_rechecked.load(Ordering::Relaxed),
            if decode_ahead { lost.len() as u64 } else { 0 }
        );
    }
}

/// The write-back case at RustyGet's capture: the article cache dropped an article's bytes
/// before its write landed (a write queued behind a flush), so the capture takes it as
/// written and par-rs reads the preallocated zeros. Since par-rs 2524277 (#145) the slice
/// waits instead of counting as damaged, because the zeros do not match the CRC of the
/// bytes the article will be written with; the cache's hand-over after the write
/// (`articles_written`) makes the next drain run `complete_staged_slices`, which verifies
/// it. No damage is ever reported and no recovery volume is asked for.
#[test]
fn an_article_taken_as_written_before_its_write_lands_is_not_damaged() {
    let f = fixture(0x7a75_0020, 6, 2, 2);
    let path = f.dir.join("part01.rar");
    std::fs::write(&path, vec![0u8; f.data.len()]).unwrap();
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let half = SLICE / 2;
    let identity = (None, &b"part01.rar"[..], &b""[..]);
    let pieces: Vec<(u64, &[u8])> = f.data[..2 * SLICE]
        .chunks(half)
        .enumerate()
        .map(|(i, c)| ((i * half) as u64, c))
        .collect();
    for &(offset, piece) in &pieces {
        streaming_session_defer_committed_article(
            f.nzb_id,
            identity,
            offset,
            piece.len() as u64,
            par_rs::crc::crc32(piece),
            Some(&path),
            || None,
        )
        .unwrap()
        .unwrap();
    }
    streaming_session_drain_articles(f.nzb_id).unwrap().unwrap();
    let status = entry.session.status();
    assert_eq!(status.damaged, 0, "slices read before their write wait");
    assert_eq!(status.verified, 0);
    assert!(entry.pending_unpause_ids.lock().unwrap().is_empty());
    // The writes land, then the cache hands the written articles over.
    let mut disk = std::fs::read(&path).unwrap();
    disk[..2 * SLICE].copy_from_slice(&f.data[..2 * SLICE]);
    std::fs::write(&path, &disk).unwrap();
    streaming_session_articles_written(f.nzb_id, identity, true, &pieces);
    assert!(
        entry.staged_due.load(Ordering::Acquire),
        "the flush asks for a staged check"
    );
    streaming_session_drain_articles(f.nzb_id).unwrap().unwrap();
    let status = entry.session.status();
    assert_eq!(status.damaged, 0, "never damaged");
    assert_eq!(status.verified, 2, "both slices verify once their bytes are on disk");
    assert!(
        entry.pending_unpause_ids.lock().unwrap().is_empty(),
        "no recovery asked for"
    );
}

/// par-rs 985b373 (#147) at RustyGet's arrival and back-fill thread: a recovery volume
/// whose exponent no slot was allocated for, arriving after slices verified, costs one
/// back-fill pass (every verified slice read once), and that pass also allocates spare
/// slots, so the next volumes of the set land in them and read nothing back.
#[test]
fn an_unplanned_volume_mid_download_costs_one_back_fill_pass() {
    // 25 slices (the last short), 2 slots preallocated (exponents 0 and 1) of a cap of 40
    // (as `parlive::resources` sizes an unplanned session), one recovery block per volume.
    let f = fixture_with(0x7a75_0021, 24, 2, 40, 8, VolumeScheme::Uniform(1));
    let entry = active_streaming_session(f.nzb_id).unwrap();
    let id = file_id(f.nzb_id);
    let lost = [1usize, 3, 5, 7, 9, 11];
    for (i, chunk) in f.data.chunks(SLICE).enumerate() {
        let offset = (i * SLICE) as u64;
        if lost.contains(&i) {
            streaming_session_on_article_failed_permanently(
                f.nzb_id,
                &id,
                offset,
                chunk.len() as u64,
            )
            .unwrap()
            .unwrap();
        } else {
            streaming_session_on_article_finished_by_name(
                f.nzb_id,
                b"part01.rar",
                offset,
                chunk.len() as u64,
                par_rs::crc::crc32(chunk),
                Some(chunk),
            )
            .unwrap()
            .unwrap();
        }
    }
    let pass = (f.data.len() - lost.len() * SLICE) as u64;
    let backfill_read = || entry.session.io_stats().backfill_bytes_read;
    let wait_backfill = || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while entry.session.needs_backfill() || entry.backfill_running.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline, "back-fill never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    let arrive = |exponent: usize| {
        let (name, volume) = &f.volumes[exponent];
        let path = f.dir.join(name);
        std::fs::write(&path, volume).unwrap();
        streaming_session_on_recovery_volume_arrived(
            f.nzb_id,
            &path,
            Some(500 + exponent as u64),
        )
        .unwrap()
        .unwrap();
    };
    // Volumes for the preallocated slots: nothing to back-fill.
    arrive(0);
    arrive(1);
    assert!(!entry.session.needs_backfill());
    assert_eq!(backfill_read(), 0);
    // An unplanned exponent: one pass, which also fills the spare slots.
    arrive(2);
    wait_backfill();
    assert_eq!(backfill_read(), pass, "one pass over the verified slices");
    let slots = entry.session.status().syndrome_slots;
    assert!(slots >= 32, "the pass allocated spares: {slots} slots");
    // The next volumes land in spares: no further pass.
    for exponent in 3..6 {
        arrive(exponent);
        wait_backfill();
    }
    assert_eq!(backfill_read(), pass, "later volumes landed in spares");
    // Six recovery blocks for six damaged slices: decode-ahead may already have rebuilt them.
    assert_eq!(entry.session.status().ready_recovery, 6);
}
