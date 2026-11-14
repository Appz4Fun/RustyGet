use super::*;
use crate::parlive::{resources, BudgetPool};
use std::sync::Arc;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rustyget-live-registry-{}-{nonce}",
            std::process::id()
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
    fn set(&self) -> PacketSet {
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
    let entry = build_streaming_session(&set, &fx.0, r.options, lease).unwrap();
    install_streaming_session(-1900101, entry);
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
        build_streaming_session(&set, &fx.0, r.options, lease).unwrap()
    };
    let old = create();
    install_streaming_session(-1900102, Arc::clone(&old));
    let current = create();
    install_streaming_session(-1900102, Arc::clone(&current));
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
