//! Trusted slice verification (par-rs `set_trust_verified_slices`), recovery volumes from
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
            volumes: VolumeScheme::Single,
            creator: "rustyget-test".to_string(),
            unicode_names: false,
            kernel: Kernel::detect(),
        },
    )
    .unwrap();
    let index = dir.join("trust.par2");
    std::fs::write(&index, &built.index).unwrap();
    std::fs::write(dir.join("part01.rar"), &data).unwrap();
    streaming_session_start(
        nzb_id,
        &index,
        &dir,
        SessionOptions {
            preallocated_syndrome_slots: slots,
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
/// before its write reads the preallocated zeros and shows the slice damaged. RustyGet's
/// capture never reports a committed article before its write (test above).
#[test]
fn a_committed_range_reported_before_its_write_reads_zeros() {
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
            .any(|e| e.state == SliceState::Damaged),
        "the zero gap fails the slice"
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
