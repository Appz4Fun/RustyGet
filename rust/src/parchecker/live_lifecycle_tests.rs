use super::*;
use crate::parlive::{resources, BudgetPool};
use std::sync::Arc;

static NEXT_FIXTURE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(super) struct Fixture(pub(super) PathBuf);
impl Fixture {
    pub(super) fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rustyget-live-registry-{}-{nonce}-{}",
            std::process::id(),
            NEXT_FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        for f in std::fs::read_dir(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/testdata/parchecker"),
        )
        .unwrap()
        {
            let f = f.unwrap();
            std::fs::copy(f.path(), dir.join(f.file_name())).unwrap();
        }
        Self(dir)
    }
    pub(super) fn set(&self) -> PacketSet {
        load_par2_set(&self.0.join("testfile.par2")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn live_registry_lease_survives_forget_until_inflight_owner_drops() {
    let fx = Fixture::new();
    let set = fx.set();
    let r = resources(set.main.as_ref().unwrap().slice_size, 128 * 1024 * 1024).unwrap();
    let bytes = r.reservation_bytes;
    let pool = Arc::new(BudgetPool::default());
    let lease = pool.acquire(bytes, bytes).unwrap();
    let entry = build_streaming_session(
        &set,
        &fx.0,
        r.options,
        lease,
        crate::parlive::download_writes(-1900101, &fx.0).unwrap(),
    )
    .unwrap();
    install_streaming_session(-1900101, entry).unwrap();
    let operation = active_streaming_session(-1900101).unwrap();
    streaming_session_forget(-1900101);
    assert!(operation.session.is_cancelled());
    assert!(pool.acquire(1, bytes).is_none());
    drop(operation);
    assert!(pool.acquire(1, bytes).is_some());
}

#[test]
fn live_registry_replacement_invalidates_old_generation() {
    let fx = Fixture::new();
    let set = fx.set();
    let pool = Arc::new(BudgetPool::default());
    let create = || {
        let r = resources(set.main.as_ref().unwrap().slice_size, 128 * 1024 * 1024).unwrap();
        let lease = pool
            .acquire(r.reservation_bytes, 500 * 1024 * 1024)
            .unwrap();
        build_streaming_session(
            &set,
            &fx.0,
            r.options,
            lease,
            crate::parlive::download_writes(-1900102, &fx.0).unwrap(),
        )
        .unwrap()
    };
    let old = create();
    install_streaming_session(-1900102, Arc::clone(&old)).unwrap();
    let current = create();
    install_streaming_session(-1900102, Arc::clone(&current)).unwrap();
    assert_ne!(old.generation, current.generation);
    assert!(old.session.is_cancelled());
    assert!(ensure_current_generation(-1900102, &old).is_err());
    assert!(ensure_current_generation(-1900102, &current).is_ok());
    streaming_session_forget(-1900102);
}

#[test]
fn live_registry_late_index_catches_existing_files_without_rereading_verified_slices() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900103;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 * 1024 * 1024)
        .unwrap();
    let mut physical_bytes = 0;
    for file_id in &set.main.as_ref().unwrap().recoverable_file_ids {
        let file = &set.file_descs[file_id];
        let path = fx.0.join(file.local_name());
        physical_bytes += std::fs::metadata(&path).unwrap().len();
        streaming_session_reconcile_file(
            id,
            *file_id,
            &path,
            &[ByteRange::new(0, file.file_length as i64)],
        )
        .unwrap()
        .unwrap();
    }
    let state = streaming_session_status(id).unwrap();
    let first = streaming_session_io_stats(id).unwrap();
    assert_eq!(state.verified, state.total);
    assert_eq!(first.source_bytes_read, physical_bytes);
    for file_id in &set.main.as_ref().unwrap().recoverable_file_ids {
        let file = &set.file_descs[file_id];
        streaming_session_reconcile_file(
            id,
            *file_id,
            &fx.0.join(file.local_name()),
            &[ByteRange::new(0, file.file_length as i64)],
        )
        .unwrap()
        .unwrap();
    }
    let second = streaming_session_io_stats(id).unwrap();
    assert_eq!(second.source_bytes_read, first.source_bytes_read);
    assert_eq!(second.ram_bytes_verified, first.ram_bytes_verified);
    streaming_session_forget(id);
}

#[test]
fn live_bounded_metadata_rejects_excess_before_parsing() {
    // A framed FileDesc larger than the entire metadata allowance must be
    // refused before its payload is allocated or decoded.
    use std::io::Write;
    let fx = Fixture::new();
    let path = fx.0.join("oversized.par2");
    let mut header = [0u8; 64];
    header[..8].copy_from_slice(b"PAR2\0PKT");
    header[8..16].copy_from_slice(&(9u64 * 1024 * 1024).to_le_bytes());
    header[48..64].copy_from_slice(b"PAR 2.0\0FileDesc");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(&header).unwrap();
    file.set_len(9 * 1024 * 1024).unwrap();
    let error = load_bounded_live_metadata(&path).unwrap_err();
    assert!(matches!(error, RepairError::StreamingMemoryLimit { .. }));
}

#[test]
fn live_bounded_metadata_preserves_the_triggering_recovery_set() {
    let fx = Fixture::new();
    let set = load_bounded_live_metadata(&fx.0.join("testfile.vol00+1.PAR2")).unwrap();
    assert!(set.main.is_some());
    assert!(!set.file_descs.is_empty());
    assert!(!set.ifscs.is_empty());
    assert_eq!(set.recovery.len(), 1);
}

#[test]
fn live_catchup_partial_ranges_and_failures_remain_visible() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900104;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let desc = &set.file_descs[&fid];
    let path = fx.0.join(desc.local_name());
    let size = set.main.as_ref().unwrap().slice_size;
    streaming_session_reconcile_file(id, fid, &path, &[ByteRange::new(0, (size / 2) as i64)])
        .unwrap()
        .unwrap();
    assert_eq!(streaming_session_status(id).unwrap().verified, 0);
    let before = streaming_session_io_stats(id).unwrap().source_bytes_read;
    assert!(
        streaming_session_reconcile_file(id, fid, &path, &[ByteRange::new(i64::MAX, 10)])
            .unwrap()
            .is_err()
    );
    assert!(streaming_session_last_error(id).is_some());
    assert_eq!(
        streaming_session_io_stats(id).unwrap().source_bytes_read,
        before
    );
    assert!(streaming_session_on_article_finished_by_name(
        id,
        b"not-a-PAR-source",
        0,
        4,
        0,
        Some(&[0; 4])
    )
    .unwrap()
    .is_err());
    assert!(streaming_session_last_error(id)
        .unwrap()
        .contains("recovery set"));
    streaming_session_cancel(id);
    assert!(matches!(
        streaming_session_reconcile_file(id, fid, &path, &[]),
        Some(Err(RepairError::Cancelled))
    ));
    streaming_session_forget(id);
}

#[test]
fn live_owned_article_queue_consumes_snapshots_without_source_reads() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900105;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let mut expected = 0;
    for fid in &set.main.as_ref().unwrap().recoverable_file_ids {
        let desc = &set.file_descs[fid];
        let path = fx.0.join(desc.local_name());
        let bytes = std::fs::read(&path).unwrap();
        expected += bytes.len() as u64;
        streaming_session_defer_article(
            id,
            (&desc.name, &[]),
            0,
            bytes.len() as u64,
            crc32fast::hash(&bytes),
            || Some(bytes),
        )
        .unwrap()
        .unwrap();
        std::fs::remove_file(path).unwrap();
    }
    assert_eq!(
        streaming_session_status(id).unwrap().verified,
        0,
        "the queue-lock capture path must never hash or fold"
    );
    streaming_session_drain_articles(id).unwrap().unwrap();
    let status = streaming_session_status(id).unwrap();
    assert_eq!(status.verified, status.total);
    let stats = streaming_session_io_stats(id).unwrap();
    assert_eq!(stats.source_bytes_read, 0);
    assert_eq!(stats.ram_bytes_verified, expected);
    streaming_session_forget(id);
}

#[test]
fn live_owned_article_queue_refuses_oversized_copy_and_exposes_cache_miss() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900106;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let name = &set.file_descs[&fid].name;
    let mut copied = false;
    assert!(
        streaming_session_defer_article(id, (name, &[]), 0, 129 << 20, 0, || {
            copied = true;
            None
        })
        .unwrap()
        .is_err()
    );
    assert!(!copied, "admission must precede a cache copy");
    assert!(
        streaming_session_defer_article(id, (name, &[]), 0, 4, 0, || None)
            .unwrap()
            .is_err()
    );
    assert!(streaming_session_last_error(id).unwrap().contains("cache"));
    assert!(streaming_session_take_warning(id).is_some());
    assert!(streaming_session_take_warning(id).is_none());
    assert_eq!(streaming_session_io_stats(id).unwrap().source_bytes_read, 0);
    streaming_session_forget(id);
}

#[test]
fn live_owned_article_queue_preserves_failed_then_successful_retry() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900107;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let desc = &set.file_descs[&fid];
    let data = std::fs::read(fx.0.join(desc.local_name())).unwrap();
    streaming_session_defer_failure(id, (&desc.name, &[]), 0, 4)
        .unwrap()
        .unwrap();
    assert_eq!(streaming_session_status(id).unwrap().damaged, 0);
    streaming_session_drain_articles(id).unwrap().unwrap();
    assert_eq!(streaming_session_status(id).unwrap().damaged, 1);
    streaming_session_defer_article(
        id,
        (&desc.name, &[]),
        0,
        data.len() as u64,
        crc32fast::hash(&data),
        || Some(data),
    )
    .unwrap()
    .unwrap();
    streaming_session_drain_articles(id).unwrap().unwrap();
    assert_eq!(streaming_session_status(id).unwrap().damaged, 0);
    assert_eq!(streaming_session_io_stats(id).unwrap().source_bytes_read, 0);
    streaming_session_forget(id);
}

#[test]
fn live_file_completion_queue_starts_from_first_par_and_replays_committed_sources() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900108;
    let par = LiveCompletedFile {
        nzb_id: id,
        base_dir: fx.0.clone(),
        path: fx.0.join("testfile.vol00+1.PAR2"),
        name: b"testfile.vol00+1.PAR2".to_vec(),
        hash16k: Vec::new(),
        par_file: true,
        successful: true,
        accepted: Vec::new(),
        queued_id: Some(1),
    };
    streaming_session_queue_completed_file(id, 4096, || Ok(par)).unwrap();
    let started = streaming_session_process_completed_file(id, |_| Ok(())).unwrap();
    assert!(started.unwrap());
    assert_eq!(streaming_session_status(id).unwrap().ready_recovery, 1);
    for fid in &set.main.as_ref().unwrap().recoverable_file_ids {
        let desc = &set.file_descs[fid];
        let source = LiveCompletedFile {
            nzb_id: id,
            base_dir: fx.0.clone(),
            path: fx.0.join(desc.local_name()),
            name: desc.name.clone(),
            hash16k: Vec::new(),
            par_file: false,
            successful: true,
            accepted: Vec::new(),
            queued_id: None,
        };
        streaming_session_queue_completed_file(id, 4096, || Ok(source)).unwrap();
        assert!(!streaming_session_process_completed_file(id, |_| Ok(()))
            .unwrap()
            .unwrap());
    }
    let status = streaming_session_status(id).unwrap();
    assert_eq!(status.verified, status.total);
    let before = streaming_session_io_stats(id).unwrap().source_bytes_read;
    assert!(before > 0);
    streaming_session_forget(id);
}

#[test]
fn damaged_recovery_volume_still_delivers_its_good_blocks() {
    let fx = Fixture::new();
    let id = -1900199;
    let par = LiveCompletedFile {
        nzb_id: id,
        base_dir: fx.0.clone(),
        path: fx.0.join("testfile.vol00+1.PAR2"),
        name: b"testfile.vol00+1.PAR2".to_vec(),
        hash16k: Vec::new(),
        par_file: true,
        successful: true,
        accepted: Vec::new(),
        queued_id: Some(1),
    };
    streaming_session_queue_completed_file(id, 4096, || Ok(par)).unwrap();
    assert!(streaming_session_process_completed_file(id, |_| Ok(()))
        .unwrap()
        .unwrap());
    assert_eq!(streaming_session_status(id).unwrap().ready_recovery, 1);
    // A lost article zero-fills part of the second of three recovery packets.
    let vol = fx.0.join("testfile.vol03+3.PAR2");
    let mut bytes = std::fs::read(&vol).unwrap();
    let magic = b"PAR 2.0\0RecvSlic";
    let starts: Vec<usize> = (0..bytes.len().saturating_sub(16))
        .filter(|&i| &bytes[i..i + 16] == magic)
        .collect();
    assert_eq!(starts.len(), 3);
    let hole = starts[1] + 64;
    bytes[hole..hole + 256].fill(0);
    std::fs::write(&vol, &bytes).unwrap();
    let damaged = LiveCompletedFile {
        nzb_id: id,
        base_dir: fx.0.clone(),
        path: vol,
        name: b"testfile.vol03+3.PAR2".to_vec(),
        hash16k: Vec::new(),
        par_file: true,
        successful: false,
        accepted: Vec::new(),
        queued_id: Some(4),
    };
    streaming_session_queue_completed_file(id, 4096, || Ok(damaged)).unwrap();
    assert!(!streaming_session_process_completed_file(id, |_| Ok(()))
        .unwrap()
        .unwrap());
    // Slots follow the planned exponents (no damage: 0, 1, 2): the two good blocks of
    // vol03+3 are absorbed or, without a slot, kept as turned away for growth.
    let entry = active_streaming_session(id).unwrap();
    assert!(
        entry.session.status().ready_recovery + entry.session.turned_away_recovery_slices()
            >= 3
    );
    drop(entry);
    streaming_session_forget(id);
}

#[test]
fn live_partial_completed_ranges_do_not_accept_sparse_holes() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900109;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let desc = &set.file_descs[&fid];
    let slice = set.main.as_ref().unwrap().slice_size;
    let source = LiveCompletedFile {
        nzb_id: id,
        base_dir: fx.0.clone(),
        path: fx.0.join(desc.local_name()),
        name: desc.name.clone(),
        hash16k: Vec::new(),
        par_file: false,
        successful: false,
        accepted: vec![ByteRange::new(0, (slice / 2) as i64)],
        queued_id: None,
    };
    streaming_session_queue_completed_file(id, 4096, || Ok(source)).unwrap();
    streaming_session_process_completed_file(id, |_| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(streaming_session_status(id).unwrap().verified, 0);
    assert!(streaming_session_status(id).unwrap().damaged > 0);
    assert_eq!(streaming_session_io_stats(id).unwrap().source_bytes_read, 0);
    streaming_session_forget(id);
}

#[test]
fn live_completed_damage_history_excludes_known_holes() {
    use crate::dupefallback::NzbgetRsByteRange;
    let id = -1900110;
    crate::pardamage::file_completed(
        id,
        false,
        1,
        4096,
        2,
        4096,
        b"part40.rar",
        &[NzbgetRsByteRange {
            offset: 1024,
            size: 1024,
        }],
    );
    let accepted = crate::pardamage::completed_accepted_ranges(id, b"part40.rar", 4096).unwrap();
    assert_eq!(
        accepted,
        vec![ByteRange::new(0, 1024), ByteRange::new(2048, 2048)]
    );
    assert!(crate::pardamage::completed_accepted_ranges(id, b"part40.rar", 8192).is_none());
    crate::pardamage::forget(id);
}

#[test]
fn live_obfuscated_article_uses_checked_hash_mapping_and_owned_ram() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900111;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let desc = &set.file_descs[&fid];
    let data = std::fs::read(fx.0.join(desc.local_name())).unwrap();
    let bytes = data.len() as u64;
    let hash: String = desc.hash_16k.iter().map(|b| format!("{b:02x}")).collect();
    streaming_session_defer_article(
        id,
        (b"obfuscated-name", hash.as_bytes()),
        0,
        bytes,
        crc32fast::hash(&data),
        || Some(data),
    )
    .unwrap()
    .unwrap();
    std::fs::remove_file(fx.0.join(desc.local_name())).unwrap();
    streaming_session_drain_articles(id).unwrap().unwrap();
    assert!(streaming_session_status(id).unwrap().verified > 0);
    assert_eq!(
        streaming_session_io_stats(id).unwrap().ram_bytes_verified,
        bytes
    );
    assert_eq!(streaming_session_io_stats(id).unwrap().source_bytes_read, 0);
    streaming_session_forget(id);
}

#[test]
fn live_obfuscated_completed_source_reads_its_committed_path() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900112;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let desc = &set.file_descs[&fid];
    let path = fx.0.join("obfuscated.dat");
    std::fs::rename(fx.0.join(desc.local_name()), &path).unwrap();
    let hash: String = desc.hash_16k.iter().map(|b| format!("{b:02x}")).collect();
    let source = LiveCompletedFile {
        nzb_id: id,
        base_dir: fx.0.clone(),
        path,
        name: b"obfuscated.dat".to_vec(),
        hash16k: hash.into_bytes(),
        par_file: false,
        successful: true,
        accepted: Vec::new(),
        queued_id: None,
    };
    streaming_session_queue_completed_file(id, 4096, || Ok(source)).unwrap();
    streaming_session_process_completed_file(id, |_| Ok(()))
        .unwrap()
        .unwrap();
    assert!(streaming_session_status(id).unwrap().verified > 0);
    assert_eq!(
        streaming_session_io_stats(id).unwrap().source_bytes_read,
        desc.file_length
    );
    streaming_session_forget(id);
}

#[test]
fn live_completed_file_resolves_renamed_path_after_capture() {
    let fx = Fixture::new();
    let id = -1900113;
    let old = fx.0.join("testfile.par2");
    let renamed = fx.0.join("renamed-index.par2");
    let par = LiveCompletedFile {
        nzb_id: id,
        base_dir: fx.0.clone(),
        path: old.clone(),
        name: b"testfile.par2".to_vec(),
        hash16k: Vec::new(),
        par_file: true,
        successful: true,
        accepted: Vec::new(),
        queued_id: Some(1),
    };
    streaming_session_queue_completed_file(id, 4096, || Ok(par)).unwrap();
    std::fs::rename(old, &renamed).unwrap();
    assert!(streaming_session_process_completed_file(id, |file| {
        file.path = renamed;
        file.name = b"renamed-index.par2".to_vec();
        Ok(())
    })
    .unwrap()
    .unwrap());
    assert!(streaming_session_has_session(id));
    streaming_session_forget(id);
}

#[test]
fn live_completion_replay_has_one_owner_per_download() {
    let owner = streaming_session_claim_completion_drain(-1900114).unwrap();
    assert!(streaming_session_claim_completion_drain(-1900114).is_none());
    assert!(
        streaming_session_claim_completion_drain(-1900114).is_none(),
        "a rejected claim must not release the current owner's lease"
    );
    let other = streaming_session_claim_completion_drain(-1900115).unwrap();
    drop(owner);
    assert!(streaming_session_claim_completion_drain(-1900114).is_some());
    drop(other);
}

#[test]
fn closed_startup_ticket_cannot_install_a_session_after_cleanup() {
    let fx = Fixture::new();
    let id = -1900116;
    let par = LiveCompletedFile {
        nzb_id: id,
        base_dir: fx.0.clone(),
        path: fx.0.join("testfile.par2"),
        name: b"testfile.par2".to_vec(),
        hash16k: Vec::new(),
        par_file: true,
        successful: true,
        accepted: Vec::new(),
        queued_id: Some(1),
    };
    streaming_session_queue_completed_file(id, 4096, || Ok(par)).unwrap();
    crate::parlive::forget_download_writes(id);
    assert!(matches!(
        streaming_session_process_completed_file(id, |_| Ok(())),
        Some(Err(RepairError::Cancelled))
    ));
    assert!(!streaming_session_has_session(id));
    streaming_session_forget(id);
}

#[test]
fn live_target_lease_repairs_real_bytes_and_seals_the_same_queue_file() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900140;
    streaming_session_start_bounded(id, &fx.0.join("testfile.vol00+1.PAR2"), &fx.0, 500 << 20)
        .unwrap();
    let first = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let ss = set.main.as_ref().unwrap().slice_size;
    let desc = &set.file_descs[&first];
    let target = fx.0.join(desc.local_name());
    let original = std::fs::read(&target).unwrap();
    for fid in &set.main.as_ref().unwrap().recoverable_file_ids {
        let file = &set.file_descs[fid];
        let start = if *fid == first {
            ss.min(file.file_length)
        } else {
            0
        };
        streaming_session_reconcile_file(
            id,
            *fid,
            &fx.0.join(file.local_name()),
            &[ByteRange::new(
                start as i64,
                (file.file_length - start) as i64,
            )],
        )
        .unwrap()
        .unwrap();
    }
    use std::io::{Seek, SeekFrom, Write};
    let mut disk = std::fs::OpenOptions::new()
        .write(true)
        .open(&target)
        .unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    disk.write_all(&vec![0; (ss as usize).min(original.len())])
        .unwrap();
    drop(disk);
    assert!(streaming_session_status(id).unwrap().early_cutover_possible);
    let request = streaming_session_prepare_repair(
        id,
        vec![crate::parlive::WriterTarget {
            file_id: 40,
            path: target.clone(),
        }],
        reserve_control(id, 4096, 500 << 20).unwrap(),
    )
    .unwrap();
    assert_eq!(
        request.generation(),
        active_streaming_session(id).unwrap().generation
    );
    let lease = request.acquire().unwrap();
    assert!(streaming_session_verified_target_crc(id, &fx.0, 40, &desc.name).is_none());
    let (report, released) = lease
        .repair_and_publish(
            |verified| {
                assert_eq!(verified, &[40]);
                1
            },
            || (),
        )
        .unwrap();
    assert!(!report.repaired_slices.is_empty());
    assert_eq!(released, 1);
    let metrics = streaming_session_metrics(id).unwrap();
    assert_eq!(metrics.repair.attempts, 1);
    assert_eq!(metrics.repair.completed, 1);
    assert_eq!(
        metrics.repair.target_bytes_written,
        ss.min(original.len() as u64)
    );
    assert!(metrics.repair.decode_ns > 0 && metrics.repair.target_verification_ns > 0);
    assert!(metrics.count_ready_to_handoff_ns.is_some());
    assert_eq!(std::fs::read(&target).unwrap(), original);
    assert_eq!(
        streaming_session_verified_target_crc(id, &fx.0, 40, &desc.name),
        Some(crc32fast::hash(&original))
    );
    assert!(streaming_session_verified_target_crc(id, &fx.0, 41, &desc.name).is_none());
    assert!(
        streaming_session_verified_target_crc(id, &fx.0.join("other"), 40, &desc.name).is_none()
    );
    let source = crate::parlive::source_write_context(id, 40, &fx.0, &desc.name).unwrap();
    assert!(matches!(
        source.permit(),
        Err(crate::parlive::WriteBlocked::Certified)
    ));
    streaming_session_cancel(id);
    assert!(streaming_session_verified_target_crc(id, &fx.0, 40, &desc.name).is_none());
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn live_target_rejects_replacement_after_source_attestation() {
    use std::io::{Seek, SeekFrom, Write};
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1_961_700;
    streaming_session_start_bounded(id, &fx.0.join("testfile.vol00+1.PAR2"), &fx.0, 500 << 20).unwrap();
    let first = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let ss = set.main.as_ref().unwrap().slice_size;
    let desc = &set.file_descs[&first];
    let target = fx.0.join(desc.local_name());
    let original = std::fs::read(&target).unwrap();
    for fid in &set.main.as_ref().unwrap().recoverable_file_ids {
        let file = &set.file_descs[fid];
        let start = if *fid == first { ss.min(file.file_length) } else { 0 };
        streaming_session_reconcile_file(id, *fid, &fx.0.join(file.local_name()),
            &[ByteRange::new(start as i64, (file.file_length - start) as i64)]).unwrap().unwrap();
    }
    let mut disk = std::fs::OpenOptions::new().write(true).open(&target).unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    disk.write_all(&vec![0; (ss as usize).min(original.len())]).unwrap();
    drop(disk);
    let damaged = std::fs::read(&target).unwrap();
    let context = crate::parlive::source_write_context(id, 40, &fx.0, &desc.name).unwrap();
    context.attest_existing_source().unwrap();
    let request = streaming_session_prepare_repair(id,
        vec![crate::parlive::WriterTarget { file_id: 40, path: target.clone() }],
        reserve_control(id, 4096, 500 << 20).unwrap()).unwrap();
    let lease = request.acquire().unwrap();
    let old = fx.0.join("original-inode-held");
    std::fs::rename(&target, &old).unwrap();
    std::fs::write(&target, &damaged).unwrap();
    let mut published = false;
    let result = lease.repair_and_publish(|_| { published = true; 1 }, || ());
    assert_eq!(std::fs::read(&target).unwrap(), damaged, "replacement inode was modified");
    assert_eq!(std::fs::read(old).unwrap(), damaged, "old inode was modified after the path changed");
    assert!(!published, "replaced source released a hold");
    assert!(result.is_err());
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn cancelled_target_lease_never_publishes_a_hold_release() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900191;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let target = fx.0.join(set.file_descs[&fid].local_name());
    let request = streaming_session_prepare_repair(
        id,
        vec![crate::parlive::WriterTarget {
            file_id: 40,
            path: target,
        }],
        reserve_control(id, 4096, 500 << 20).unwrap(),
    )
    .unwrap();
    assert_eq!(
        request.generation(),
        active_streaming_session(id).unwrap().generation
    );
    let lease = request.acquire().unwrap();
    streaming_session_cancel(id);
    assert!(matches!(
        lease.repair_and_publish(|_| panic!("cancelled repair released a hold"), || ()),
        Err(RepairError::Cancelled)
    ));
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn live_cancellation_and_hold_publication_share_one_fence() {
    let fx = Fixture::new();
    let id = -1900192;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let entry = active_streaming_session(id).unwrap();
    let gate = Arc::clone(&entry.writers);
    let ownership = gate
        .begin_repair(
            entry.generation,
            vec![crate::parlive::WriterTarget {
                file_id: 40,
                path: fx.0.join("testfile.dat"),
            }],
            reserve_control(id, 4096, 500 << 20).unwrap(),
        )
        .unwrap();
    let (inside_tx, inside_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let publication = std::thread::spawn(move || {
        ownership.publish(&[(40, 0)], || {
            inside_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        })
    });
    inside_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let cancellation = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        streaming_session_cancel(id);
    });
    started_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_millis(100);
    while !entry.session.is_cancelled() && std::time::Instant::now() < until {
        std::thread::yield_now();
    }
    let cancelled_inside_publication = entry.session.is_cancelled();
    release_tx.send(()).unwrap();
    publication.join().unwrap().unwrap();
    cancellation.join().unwrap();
    assert!(
        !cancelled_inside_publication,
        "library cancellation raced an already-owned hold publication"
    );
    assert!(entry.session.is_cancelled());
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn forgotten_completion_drops_metadata_admission_and_cannot_restart() {
    let fx = Fixture::new();
    let id = -1900143;
    let par = LiveCompletedFile {
        nzb_id: id,
        base_dir: fx.0.clone(),
        path: fx.0.join("testfile.par2"),
        name: b"testfile.par2".to_vec(),
        hash16k: Vec::new(),
        par_file: true,
        successful: true,
        accepted: Vec::new(),
        queued_id: Some(1),
    };
    streaming_session_queue_completed_file(id, 4096, || Ok(par)).unwrap();
    streaming_session_forget(id);
    assert!(
        !streaming_session_pending_completed_jobs().contains(&id),
        "forgotten completion metadata still owns a startup ticket"
    );
    assert!(
        streaming_session_process_completed_file(id, |_| panic!("forgotten ticket ran")).is_none()
    );
    assert!(!streaming_session_has_session(id));
    crate::parlive::forget_download_writes(id);
}

#[test]
fn live_metrics_distinguish_network_capture_from_committed_replay() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900144;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let files = &set.main.as_ref().unwrap().recoverable_file_ids;
    assert_eq!(files.len(), 2);
    let first = &set.file_descs[&files[0]];
    let bytes = std::fs::read(fx.0.join(first.local_name())).unwrap();
    let accepted = bytes.len() as u64;
    streaming_session_defer_article(
        id,
        (&first.name, &[]),
        0,
        accepted,
        crc32fast::hash(&bytes),
        || Some(bytes),
    )
    .unwrap()
    .unwrap();
    streaming_session_drain_articles(id).unwrap().unwrap();
    let before = streaming_session_metrics(id).unwrap();
    assert_eq!(before.captured_articles, 1);
    assert_eq!(before.captured_bytes, accepted);
    assert_eq!(before.committed_replay_bytes, 0);
    assert!(before.count_ready_at.is_none());
    let second = &set.file_descs[&files[1]];
    let replay_start = std::time::Instant::now();
    streaming_session_reconcile_file(
        id,
        files[1],
        &fx.0.join(second.local_name()),
        &[ByteRange::new(0, second.file_length as i64)],
    )
    .unwrap()
    .unwrap();
    let replay_end = std::time::Instant::now();
    let after = streaming_session_metrics(id).unwrap();
    assert_eq!(after.captured_articles, 1);
    assert_eq!(after.captured_bytes, accepted);
    assert_eq!(after.committed_replay_bytes, second.file_length);
    assert_eq!(after.io.ram_bytes_verified, accepted + second.file_length);
    assert_eq!(after.repair.attempts, 0);
    assert!(after
        .count_ready_at
        .is_some_and(|time| time >= replay_start && time <= replay_end));
    let line = after.log_line(id, "test\"event");
    let json = crate::json::parse(
        &line[b"Live PAR metrics ".len()..],
        crate::json::TEST_LIMITS,
    )
    .unwrap();
    assert_eq!(
        json.get(b"event"),
        Some(&crate::json::Value::String(b"test\"event".to_vec()))
    );
    assert_eq!(
        json.get(b"count_ready_to_handoff_ns"),
        Some(&crate::json::Value::Null)
    );
    assert_eq!(
        json.get(b"captured_bytes").unwrap().to_f64(),
        Some(accepted as f64)
    );
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn live_metrics_do_not_count_unavailable_cache_as_network_capture() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900145;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let desc = &set.file_descs[&set.main.as_ref().unwrap().recoverable_file_ids[0]];
    assert!(
        streaming_session_defer_article(id, (&desc.name, &[]), 0, desc.file_length, 0, || None)
            .unwrap()
            .is_err()
    );
    let metrics = streaming_session_metrics(id).unwrap();
    assert_eq!(metrics.captured_articles, 0);
    assert_eq!(metrics.captured_bytes, 0);
    assert_eq!(metrics.cache_misses, 1);
    assert_eq!(metrics.capture_refusals, 1);
    assert_eq!(metrics.io.ram_bytes_verified, 0);
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn live_target_lease_consumes_the_final_owned_article_before_postprocessing() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900146;
    streaming_session_start_bounded(id, &fx.0.join("testfile.vol00+1.PAR2"), &fx.0, 500 << 20)
        .unwrap();
    let files = &set.main.as_ref().unwrap().recoverable_file_ids;
    let target_id = *files
        .iter()
        .max_by_key(|fid| set.file_descs[fid].file_length)
        .unwrap();
    let last_id = *files.iter().find(|fid| **fid != target_id).unwrap();
    let target_desc = &set.file_descs[&target_id];
    let target = fx.0.join(target_desc.local_name());
    let original = std::fs::read(&target).unwrap();
    let ss = set.main.as_ref().unwrap().slice_size;
    streaming_session_reconcile_file(
        id,
        target_id,
        &target,
        &[ByteRange::new(
            ss as i64,
            (target_desc.file_length - ss) as i64,
        )],
    )
    .unwrap()
    .unwrap();
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&target)
        .unwrap()
        .write_all(&vec![0; ss as usize])
        .unwrap();
    let last = &set.file_descs[&last_id];
    let bytes = std::fs::read(fx.0.join(last.local_name())).unwrap();
    streaming_session_defer_article(
        id,
        (&last.name, &[]),
        0,
        bytes.len() as u64,
        crc32fast::hash(&bytes),
        || Some(bytes),
    )
    .unwrap()
    .unwrap();
    assert!(
        !streaming_session_status(id).unwrap().early_cutover_possible,
        "the queue callback observes pre-fold state"
    );
    assert!(streaming_session_can_schedule_repair(id));
    let request = streaming_session_prepare_repair(
        id,
        vec![crate::parlive::WriterTarget {
            file_id: 40,
            path: target.clone(),
        }],
        reserve_control(id, 4096, 500 << 20).unwrap(),
    )
    .unwrap();
    let (_, released) = request
        .acquire()
        .unwrap()
        .repair_and_publish(
            |verified| {
                assert_eq!(verified, &[40]);
                1
            },
            || (),
        )
        .unwrap();
    assert_eq!(released, 1);
    assert_eq!(std::fs::read(target).unwrap(), original);
    assert_eq!(
        streaming_session_metrics(id).unwrap().captured_bytes,
        last.file_length
    );
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn refused_helper_finishes_before_article_drainers_can_take_the_operation() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900147;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let request = streaming_session_prepare_repair(
        id,
        vec![crate::parlive::WriterTarget {
            file_id: 40,
            path: fx.0.join(set.file_descs[&fid].local_name()),
        }],
        reserve_control(id, 4096, 500 << 20).unwrap(),
    )
    .unwrap();
    let entry = active_streaming_session(id).unwrap();
    let finished = std::sync::atomic::AtomicBool::new(false);
    let outcome = request.acquire().unwrap().repair_and_publish(
        |_| panic!("unsolvable target published"),
        || {
            assert!(matches!(
                entry.operation.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            finished.store(true, Ordering::SeqCst);
        },
    );
    assert!(matches!(
        outcome,
        Err(RepairError::NotEnoughRecovery { .. })
    ));
    assert!(finished.load(Ordering::SeqCst));
    assert!(entry.operation.try_lock().is_ok());
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn repair_control_admission_survives_until_the_publisher_metadata_drops() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900148;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let request = streaming_session_prepare_repair(
        id,
        vec![crate::parlive::WriterTarget {
            file_id: 40,
            path: fx.0.join(set.file_descs[&fid].local_name()),
        }],
        reserve_control(id, 1 << 20, 500 << 20).unwrap(),
    )
    .unwrap();
    let publisher = request.admission_hold();
    streaming_session_cancel(id);
    assert!(request.acquire().is_err());
    assert!(
        reserve_control(id, 256, 500 << 20).is_err(),
        "publisher metadata became uncharged after repair refusal"
    );
    drop(publisher);
    assert!(reserve_control(id, 256, 500 << 20).is_ok());
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn first_recovery_file_stamps_readiness_before_any_repair() {
    let fx = Fixture::new();
    let dir = fx.0.join("seeded-ready");
    std::fs::create_dir(&dir).unwrap();
    let source = dir.join("only.rar");
    std::fs::write(&source, [0x63; 512]).unwrap();
    let options = par_rs::create::CreateOptions {
        slice_size: 512,
        recovery_blocks: 1,
        volumes: par_rs::create::VolumeScheme::Uniform(1),
        engine: par_rs::EngineConfig {
            num_threads: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let created = par_rs::create::create(
        &[par_rs::create::SourceFile::new(source)],
        &dir,
        "ready",
        &options,
        None,
        &mut |_, _, _| {},
    )
    .unwrap();
    let id = -1900149;
    let started = std::time::Instant::now();
    streaming_session_start_bounded(id, &created.files[1], &dir, 500 << 20).unwrap();
    let finished = std::time::Instant::now();
    let metrics = streaming_session_metrics(id).unwrap();
    assert!(metrics.status.early_cutover_possible);
    assert_eq!(metrics.repair.attempts, 0);
    assert!(
        metrics
            .count_ready_at
            .is_some_and(|time| time >= started && time <= finished),
        "initial recovery readiness was not observed until after solve"
    );
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn live_repair_readiness_includes_the_current_drainers_owned_article() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900150;
    streaming_session_start_bounded(id, &fx.0.join("testfile.vol00+1.PAR2"), &fx.0, 500 << 20)
        .unwrap();
    let files = &set.main.as_ref().unwrap().recoverable_file_ids;
    let target_id = *files
        .iter()
        .max_by_key(|fid| set.file_descs[fid].file_length)
        .unwrap();
    let last_id = *files.iter().find(|fid| **fid != target_id).unwrap();
    let target_desc = &set.file_descs[&target_id];
    let target = fx.0.join(target_desc.local_name());
    let original = std::fs::read(&target).unwrap();
    let ss = set.main.as_ref().unwrap().slice_size;
    streaming_session_reconcile_file(
        id,
        target_id,
        &target,
        &[ByteRange::new(
            ss as i64,
            (target_desc.file_length - ss) as i64,
        )],
    )
    .unwrap()
    .unwrap();
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&target)
        .unwrap()
        .write_all(&vec![0; ss as usize])
        .unwrap();
    let last = &set.file_descs[&last_id];
    let bytes = std::fs::read(fx.0.join(last.local_name())).unwrap();
    streaming_session_defer_article(
        id,
        (&last.name, &[]),
        0,
        bytes.len() as u64,
        crc32fast::hash(&bytes),
        || Some(bytes),
    )
    .unwrap()
    .unwrap();
    assert!(streaming_session_can_schedule_repair(id));
    let entry = active_streaming_session(id).unwrap();
    let operation = entry.operation.lock().unwrap();
    // Real production dequeue: the frame is still owned and has not been folded.
    let article = take_live_article(&entry).unwrap();
    assert!(entry.articles.lock().unwrap().pending.is_empty());
    assert!(!entry.session.status().early_cutover_possible);
    assert!(
        streaming_session_can_schedule_repair(id),
        "active RAM disappeared between dequeue and IFSC/GF completion"
    );
    drop(article);
    assert!(
        !streaming_session_can_schedule_repair(id),
        "discarded RAM still authorized helper scheduling"
    );
    let bytes = std::fs::read(fx.0.join(last.local_name())).unwrap();
    streaming_session_defer_article(
        id,
        (&last.name, &[]),
        0,
        bytes.len() as u64,
        crc32fast::hash(&bytes),
        || Some(bytes),
    )
    .unwrap()
    .unwrap();
    let article = take_live_article(&entry).unwrap();
    let outcome = article.fold().unwrap();
    entry.unpause(outcome.unpause.as_ref());
    drop(article);
    drop(operation);
    assert_eq!(entry.articles.lock().unwrap().bytes, 0);
    let request = streaming_session_prepare_repair(
        id,
        vec![crate::parlive::WriterTarget {
            file_id: 40,
            path: target.clone(),
        }],
        reserve_control(id, 4096, 500 << 20).unwrap(),
    )
    .unwrap();
    request
        .acquire()
        .unwrap()
        .repair_and_publish(
            |verified| {
                assert_eq!(verified, &[40]);
                1
            },
            || (),
        )
        .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), original);
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn live_metrics_max_width_transport_survives_the_actual_native_formatter() {
    let fx = Fixture::new();
    let id = -1900151;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let mut metrics = streaming_session_metrics(id).unwrap();
    metrics.generation = SessionGeneration(u64::MAX);
    metrics.captured_articles = u64::MAX;
    metrics.captured_bytes = u64::MAX;
    metrics.capture_refusals = u64::MAX;
    metrics.cache_misses = u64::MAX;
    metrics.committed_replay_bytes = u64::MAX;
    metrics.writer_wait_ns = u64::MAX;
    metrics.operation_wait_ns = u64::MAX;
    metrics.count_ready_at = Some(std::time::Instant::now());
    metrics.count_ready_to_handoff_ns = Some(u64::MAX);
    metrics.io = LiveIoStats {
        ram_bytes_verified: u64::MAX,
        source_bytes_read: u64::MAX,
        backfill_bytes_read: u64::MAX,
        recovery_bytes_read: u64::MAX,
        target_verification_bytes_read: u64::MAX,
    };
    metrics.repair = par_rs::streaming::RepairPhaseStats {
        attempts: u64::MAX,
        completed: u64::MAX,
        target_bytes_written: u64::MAX,
        payload_wait_ns: u64::MAX,
        backfill_ns: u64::MAX,
        matrix_ns: u64::MAX,
        decode_ns: u64::MAX,
        target_write_ns: u64::MAX,
        target_sync_ns: u64::MAX,
        refold_ns: u64::MAX,
        target_verification_ns: u64::MAX,
        total_ns: u64::MAX,
    };
    metrics.status.syndrome_slots = usize::MAX;
    metrics.status.ready_recovery = usize::MAX;
    metrics.status.pending_folds = usize::MAX;
    metrics.status.verified = usize::MAX;
    metrics.status.total = usize::MAX;
    let original = metrics.log_line(i32::MIN, "repair_complete");
    assert!(
        original.len() > 1016,
        "fixture must exceed the original message capacity"
    );
    #[cfg(target_pointer_width = "64")]
    assert_eq!(
        crc32fast::hash(&original),
        3_467_611_658,
        "independent Python reference vector"
    );
    let records: Vec<_> = metrics
        .log_frames(i32::MIN, "repair_complete", u64::MAX)
        .collect();
    let mut restored = Vec::new();
    for record in &records {
        let native = crate::log::native_detail_record_for_test(record);
        assert_eq!(
            native.len(),
            record.len() + b"DETAIL\t".len(),
            "native logger truncated metrics"
        );
        let start = native.iter().position(|&b| b == b'{').unwrap();
        let json = crate::json::parse(&native[start..], crate::json::TEST_LIMITS).unwrap();
        let crate::json::Value::String(hex) = json.get(b"hex").unwrap() else {
            panic!("frame missing payload")
        };
        for pair in hex.chunks_exact(2) {
            restored.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
        }
        assert_eq!(
            json.get(b"crc32").unwrap().to_f64(),
            Some(f64::from(crc32fast::hash(&original)))
        );
    }
    assert_eq!(restored, original);
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn live_busy_completion_ticket_survives_until_later_tick() {
    let fx = Fixture::new();
    let id = -1900600;
    let file = LiveCompletedFile {
        nzb_id: id,
        base_dir: fx.0.clone(),
        path: fx.0.join("testfile.par2"),
        name: b"testfile.par2".to_vec(),
        hash16k: Vec::new(),
        par_file: true,
        successful: true,
        accepted: Vec::new(),
        queued_id: Some(1),
    };
    streaming_session_queue_completed_file(id, 4096, || Ok(file)).unwrap();
    let result = streaming_session_process_completed_file(id, |_| {
        Err(RepairError::Io {
            context: "live namespace busy".into(),
            source: std::io::Error::from(std::io::ErrorKind::WouldBlock),
        })
    });
    assert!(result.is_some());
    assert!(
        streaming_session_pending_completed_jobs().contains(&id),
        "transient busy consumed the only index completion event"
    );
    assert!(active_streaming_session(id).is_none());
    let remainder = crate::parlive::reserve_control(id, (1 << 20) - 4096, 500 << 20).unwrap();
    assert!(
        crate::parlive::reserve_control(id, 4096, 500 << 20).is_err(),
        "retry ticket lost its original metadata charge"
    );
    drop(remainder);
    assert!(streaming_session_process_completed_file(id, |_| {
        panic!("busy event was retried before a coordinator tick")
    })
    .is_none());
    streaming_session_retry_completion_tick();
    assert!(streaming_session_process_completed_file(id, |_| Ok(()))
        .unwrap()
        .unwrap());
    assert!(!streaming_session_pending_completed_jobs().contains(&id));
    assert!(active_streaming_session(id).is_some());
    streaming_session_forget(id);
}

/// Copies each source of the fixture to `<n>.out.tmp` (a DirectWrite file under another
/// name than its par2 name) and returns `(par2 name, written path, bytes)` per file.
fn written_copies(fx: &Fixture, set: &PacketSet) -> Vec<(Vec<u8>, PathBuf, Vec<u8>)> {
    let written = fx.0.join("written");
    std::fs::create_dir_all(&written).unwrap();
    let mut out = Vec::new();
    for (n, file_id) in set.main.as_ref().unwrap().recoverable_file_ids.iter().enumerate() {
        let desc = &set.file_descs[file_id];
        let bytes = std::fs::read(fx.0.join(desc.local_name())).unwrap();
        let path = written.join(format!("{n}.out.tmp"));
        std::fs::write(&path, &bytes).unwrap();
        out.push((desc.name.clone(), path, bytes));
    }
    out
}

/// Defers `bytes[from..to]` of a file as articles of `article` bytes that RAM no longer
/// holds (every snapshot misses), naming the file they were written into.
fn defer_uncached(
    id: i32,
    name: &[u8],
    path: &Path,
    bytes: &[u8],
    (from, to): (usize, usize),
    article: usize,
) {
    let mut offset = from;
    while offset < to {
        let end = (offset + article).min(to);
        streaming_session_defer_committed_article(
            id,
            (None, name, &[]),
            offset as u64,
            (end - offset) as u64,
            crc32fast::hash(&bytes[offset..end]),
            Some(path),
            || None,
        )
        .unwrap()
        .unwrap();
        offset = end;
    }
}

/// The repair's temporary files left in `dir`.
fn repair_temps(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".rustyget-repair-"))
        .collect()
}

#[test]
fn a_failed_target_admission_removes_only_its_temporary_files() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900184;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let ids = &set.main.as_ref().unwrap().recoverable_file_ids;
    let desc = &set.file_descs[&ids[0]];
    // Bind the physical root as a repair admission does.
    let context = crate::parlive::source_write_context(id, 40, &fx.0, &desc.name).unwrap();
    context.attest_existing_source().unwrap();
    let entry = active_streaming_session(id).unwrap();
    let existing = fx.0.join(desc.local_name());
    let before = std::fs::read(&existing).unwrap();
    let first = fx.0.join("missing-one.bin");
    let second = fx.0.join("missing-two.bin");
    let targets = vec![
        (first.clone(), ids[0]),
        (existing.clone(), ids[0]),
        (second.clone(), ids[0]),
    ];
    // Cancelled after the last target opened: no target name was given, no temp is left.
    let calls = std::cell::Cell::new(0);
    let cancel_at_end = || {
        calls.set(calls.get() + 1);
        calls.get() > 3
    };
    assert!(matches!(
        open_live_repair_targets(id, &entry, &targets, cancel_at_end),
        Err(RepairError::Cancelled)
    ));
    assert!(!first.exists() && !second.exists());
    assert!(repair_temps(&fx.0).is_empty());
    assert_eq!(std::fs::read(&existing).unwrap(), before);
    // Refused part way (a target outside the root): its temps go too.
    let outside = std::env::temp_dir().join("rustyget-outside-root.bin");
    let refused = vec![(first.clone(), ids[0]), (outside.clone(), ids[0])];
    assert!(open_live_repair_targets(id, &entry, &refused, || false).is_err());
    assert!(!first.exists() && !outside.exists());
    assert!(repair_temps(&fx.0).is_empty());
    // The direct renamer moves a completed file to a target's name while the admission
    // runs: it is never replaced or removed, the admission fails, and its temp goes.
    let swap = vec![(first.clone(), ids[0])];
    let moved = std::cell::Cell::new(false);
    let move_in = || {
        if !moved.get() && !repair_temps(&fx.0).is_empty() {
            std::fs::write(&first, b"the real movie").unwrap();
            moved.set(true);
        }
        false
    };
    assert!(open_live_repair_targets(id, &entry, &swap, move_in).is_err());
    assert_eq!(std::fs::read(&first).unwrap(), b"the real movie");
    assert!(repair_temps(&fx.0).is_empty());
    std::fs::remove_file(&first).unwrap();
    // Current: all open; the missing ones get their names (the same files), no temps stay.
    let files = open_live_repair_targets(id, &entry, &targets, || false).unwrap();
    assert_eq!(files.len(), 3);
    assert!(first.exists() && second.exists());
    assert!(repair_temps(&fx.0).is_empty());
    use std::os::unix::fs::MetadataExt;
    assert_eq!(
        files[0].file.metadata().unwrap().ino(),
        std::fs::metadata(&first).unwrap().ino()
    );
    assert_eq!(files[0].file.metadata().unwrap().nlink(), 1);
    drop(files);
    drop(entry);
    drop(context);
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

/// Serializes the tests that force the no-hard-links fallback.
static NO_LINKS: Mutex<()> = Mutex::new(());

struct NoLinks<'a>(#[allow(dead_code)] std::sync::MutexGuard<'a, ()>);
impl NoLinks<'_> {
    fn force() -> Self {
        let guard = NO_LINKS.lock().unwrap_or_else(|e| e.into_inner());
        crate::parlive::FORCE_NO_LINKS.store(true, Ordering::Relaxed);
        Self(guard)
    }
}
impl Drop for NoLinks<'_> {
    fn drop(&mut self) {
        crate::parlive::FORCE_NO_LINKS.store(false, Ordering::Relaxed);
    }
}

/// A live session on the fixture with its physical root bound, as a repair admission
/// binds it; the caller forgets it.
fn bound_session(fx: &Fixture, set: &PacketSet, id: i32) -> crate::parlive::SourceWriteContext {
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 << 20).unwrap();
    let desc = &set.file_descs[&set.main.as_ref().unwrap().recoverable_file_ids[0]];
    let context = crate::parlive::source_write_context(id, 40, &fx.0, &desc.name).unwrap();
    context.attest_existing_source().unwrap();
    context
}

#[test]
fn without_hard_links_a_missing_target_is_created_directly() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900185;
    let context = bound_session(&fx, &set, id);
    let entry = active_streaming_session(id).unwrap();
    let file_id = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let target = fx.0.join("direct.bin");
    let _no_links = NoLinks::force();
    // exFAT-like: no link, no no-replace rename: the name is created directly.
    let files = open_live_repair_targets(id, &entry, &[(target.clone(), file_id)], || false).unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_eq!(
        files[0].file.metadata().unwrap().ino(),
        std::fs::metadata(&target).unwrap().ino()
    );
    assert!(repair_temps(&fx.0).is_empty());
    drop(files);
    std::fs::remove_file(&target).unwrap();
    // Cancelled after the name was created: removed, it is still this admission's file.
    let calls = std::cell::Cell::new(0);
    let cancel_after_naming = || {
        calls.set(calls.get() + 1);
        calls.get() >= 3
    };
    assert!(matches!(
        open_live_repair_targets(id, &entry, &[(target.clone(), file_id)], cancel_after_naming),
        Err(RepairError::Cancelled)
    ));
    assert!(!target.exists());
    assert!(repair_temps(&fx.0).is_empty());
    // The name was replaced (the direct renamer moved the real file there) before the
    // cancel: the real file is never removed.
    let calls = std::cell::Cell::new(0);
    let replace_then_cancel = || {
        calls.set(calls.get() + 1);
        if calls.get() == 3 {
            std::fs::remove_file(&target).unwrap();
            std::fs::write(&target, b"the real movie").unwrap();
        }
        calls.get() >= 3
    };
    assert!(open_live_repair_targets(id, &entry, &[(target.clone(), file_id)], replace_then_cancel).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"the real movie");
    drop(entry);
    drop(context);
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn a_late_failure_removes_the_target_names_this_admission_gave() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900186;
    let context = bound_session(&fx, &set, id);
    let entry = active_streaming_session(id).unwrap();
    let file_id = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let one = fx.0.join("named-one.bin");
    let two = fx.0.join("named-two.bin");
    let targets = vec![(one.clone(), file_id), (two.clone(), file_id)];
    // Cancelled after both names were given (calls: 2 targets, after the loop, after naming).
    let calls = std::cell::Cell::new(0);
    let cancel_after_naming = || {
        calls.set(calls.get() + 1);
        calls.get() >= 4
    };
    assert!(matches!(
        open_live_repair_targets(id, &entry, &targets, cancel_after_naming),
        Err(RepairError::Cancelled)
    ));
    assert!(!one.exists() && !two.exists());
    assert!(repair_temps(&fx.0).is_empty());
    // A target name taken before naming: nothing is named, the other file is kept.
    std::fs::write(&two, b"already here").unwrap();
    let calls = std::cell::Cell::new(0);
    let occupy_second = || {
        calls.set(calls.get() + 1);
        false
    };
    // `two` exists, so it is opened as it is; take `one` after it was created as a temp.
    let occupy = vec![(one.clone(), file_id)];
    let taken = std::cell::Cell::new(false);
    let take_name = || {
        if !taken.get() && !repair_temps(&fx.0).is_empty() {
            std::fs::write(&one, b"someone").unwrap();
            taken.set(true);
        }
        occupy_second()
    };
    assert!(open_live_repair_targets(id, &entry, &occupy, take_name).is_err());
    assert_eq!(std::fs::read(&one).unwrap(), b"someone");
    assert_eq!(std::fs::read(&two).unwrap(), b"already here");
    assert!(repair_temps(&fx.0).is_empty());
    drop(entry);
    drop(context);
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn a_rename_onto_a_given_name_waits_for_the_rollback_and_survives() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900187;
    let context = bound_session(&fx, &set, id);
    let entry = active_streaming_session(id).unwrap();
    let file_id = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let target = fx.0.join("movie.mkv");
    let obfuscated = fx.0.join("abc123");
    std::fs::write(&obfuscated, b"the completed movie").unwrap();
    // The admission names the target, then is cancelled (the direct renamer joined); the
    // renamer moves the completed file onto that name at once, under the naming lock.
    let renamer = std::cell::RefCell::new(None);
    let calls = std::cell::Cell::new(0);
    let cancel_and_rename = || {
        calls.set(calls.get() + 1);
        if calls.get() == 3 {
            let (target, obfuscated) = (target.clone(), obfuscated.clone());
            *renamer.borrow_mut() = Some(std::thread::spawn(move || {
                crate::parlive::with_target_naming(id, || {
                    // The rollback has finished: the name it gave is gone, not this file.
                    let rolled_back = !target.exists();
                    std::fs::rename(&obfuscated, &target).unwrap();
                    rolled_back
                })
            }));
            // Give the renamer time to block on the lock the rollback holds.
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        calls.get() >= 3
    };
    assert!(matches!(
        open_live_repair_targets(id, &entry, &[(target.clone(), file_id)], cancel_and_rename),
        Err(RepairError::Cancelled)
    ));
    let rolled_back_first = renamer.borrow_mut().take().unwrap().join().unwrap();
    assert!(rolled_back_first, "the rename ran inside the rollback");
    assert_eq!(std::fs::read(&target).unwrap(), b"the completed movie");
    drop(entry);
    drop(context);
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn file_io_through_a_write_permit_never_waits_for_target_naming() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900188;
    let context = bound_session(&fx, &set, id);
    let entry = active_streaming_session(id).unwrap();
    let file_id = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let target = fx.0.join("missing-target.bin");
    let other = fx.0.join("completed.part01.rar");
    std::fs::write(&other, b"x").unwrap();
    // While the admission names its missing target (holding the naming lock), a completion
    // or cleanup removes a file through perform_io: it must finish, not wait for the lock.
    let finished = std::cell::Cell::new(None);
    let calls = std::cell::Cell::new(0);
    let io_during_naming = || {
        calls.set(calls.get() + 1);
        if calls.get() == 3 {
            let (sender, receiver) = std::sync::mpsc::channel();
            let source =
                crate::parlive::source_write_context(id, 41, &fx.0, b"completed.part01.rar")
                    .unwrap();
            let path = other.clone();
            std::thread::spawn(move || {
                let _ = sender.send(source.perform_io(|| std::fs::remove_file(&path)).is_ok());
            });
            finished.set(receiver.recv_timeout(std::time::Duration::from_secs(5)).ok());
        }
        false
    };
    let files =
        open_live_repair_targets(id, &entry, &[(target.clone(), file_id)], io_during_naming)
            .unwrap();
    assert_eq!(finished.get(), Some(true), "perform_io hung behind the naming lock");
    assert!(!other.exists());
    drop(files);
    drop(entry);
    drop(context);
    streaming_session_forget(id);
    crate::parlive::forget_download_writes(id);
}

#[test]
fn stale_repair_temps_of_earlier_daemons_are_removed() {
    let fx = Fixture::new();
    let stale = fx.0.join(".rustyget-repair-1-00000000deadbeef-7.tmp");
    // The same pid as this process, but an earlier process's token (a container restart).
    let same_pid = fx.0.join(format!(
        ".rustyget-repair-{}-00000000deadbeef-7.tmp",
        std::process::id()
    ));
    let dir = fx.0.join(".rustyget-repair-2-00000000deadbeef-7.tmp");
    let own = fx.0.join(format!(
        "{}7.tmp",
        crate::parlive::own_temp_prefix_for_test()
    ));
    let other = fx.0.join(".rustyget-repair-notes.txt");
    std::fs::write(&stale, b"").unwrap();
    std::fs::write(&same_pid, b"").unwrap();
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(&own, b"").unwrap();
    std::fs::write(&other, b"").unwrap();
    assert_eq!(crate::parlive::remove_stale_repair_temps(&fx.0), 2);
    assert!(!stale.exists() && !same_pid.exists());
    assert!(dir.is_dir(), "never a directory");
    assert!(own.exists(), "this process's own may be in use");
    assert!(other.exists());
}

#[test]
fn live_cache_miss_is_read_by_par_rs_from_the_written_file() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900180;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 * 1024 * 1024)
        .unwrap();
    let files = written_copies(&fx, &set);
    // Every article misses the cache: no refusal, no error; par-rs reads each slice from
    // the DirectWrite file (set_target_path) once the slice is complete.
    for (name, path, bytes) in &files {
        defer_uncached(id, name, path, bytes, (0, bytes.len()), 1000);
    }
    assert!(
        streaming_session_last_error(id).is_none(),
        "a cache miss must not be a refusal"
    );
    streaming_session_drain_articles(id).unwrap().unwrap();
    let status = streaming_session_status(id).unwrap();
    assert_eq!(status.verified, status.total);
    let total: u64 = files.iter().map(|(_, _, bytes)| bytes.len() as u64).sum();
    let stats = streaming_session_io_stats(id).unwrap();
    assert!(stats.source_bytes_read >= total, "par-rs read the slices from their files");
    let metrics = streaming_session_metrics(id).unwrap();
    assert_eq!(metrics.capture_refusals, 0);
    assert!(metrics.cache_misses > 0);
    let entry = active_streaming_session(id).unwrap();
    assert!(entry.io_stats_line(id).contains("0 still waiting"));
    drop(entry);
    streaming_session_forget(id);
}

#[test]
fn live_article_ram_refused_waits_for_the_cache_flush() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900181;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 * 1024 * 1024)
        .unwrap();
    let files = written_copies(&fx, &set);
    let entry = active_streaming_session(id).unwrap();
    // RAM refused every article while it was still in the cache: they wait.
    for (name, path, bytes) in &files {
        let file_id = entry.session.file_id_by_name(name).unwrap();
        entry.note_written(file_id, path);
        let mut offset = 0;
        while offset < bytes.len() {
            let end = (offset + 1000).min(bytes.len());
            let crc = crc32fast::hash(&bytes[offset..end]);
            assert!(entry.await_commit(file_id, offset as u64, (end - offset) as u64, crc));
            offset = end;
        }
    }
    streaming_session_drain_articles(id).unwrap().unwrap();
    assert_eq!(streaming_session_status(id).unwrap().verified, 0);
    assert!(entry.awaiting_commits() > 0);
    // The cache writes them out: par-rs reads them from their file.
    for (name, _, bytes) in &files {
        streaming_session_articles_committed(id, (None, name, &[]), &[(0, bytes.len() as u64)]);
    }
    streaming_session_drain_articles(id).unwrap().unwrap();
    let status = streaming_session_status(id).unwrap();
    assert_eq!(status.verified, status.total);
    assert_eq!(entry.awaiting_commits(), 0);
    drop(entry);
    streaming_session_forget(id);
}

#[test]
fn live_session_plans_recovery_slots_for_the_expected_damage() {
    let fx = Fixture::new();
    let id = -1900182;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 * 1024 * 1024)
        .unwrap();
    // No failures known yet: the margin of 2 slots, allocated up front.
    let entry = active_streaming_session(id).unwrap();
    assert_eq!(entry.session.max_syndrome_slots(), 2);
    assert_eq!(entry.session.status().syndrome_slots, 2);
    drop(entry);
    streaming_session_forget(id);
}

#[test]
fn live_cache_miss_with_wrong_downloaded_bytes_is_damage() {
    let fx = Fixture::new();
    let set = fx.set();
    let id = -1900183;
    streaming_session_start_bounded(id, &fx.0.join("testfile.par2"), &fx.0, 500 * 1024 * 1024)
        .unwrap();
    let files = written_copies(&fx, &set);
    let (name, path, bytes) = &files[0];
    let mut wrong = bytes.clone();
    for byte in wrong.iter_mut().step_by(97) {
        *byte ^= 0x5a;
    }
    std::fs::write(path, &wrong).unwrap();
    defer_uncached(id, name, path, &wrong, (0, wrong.len()), 1000);
    streaming_session_drain_articles(id).unwrap().unwrap();
    assert!(streaming_session_status(id).unwrap().damaged > 0);
    streaming_session_forget(id);
}
