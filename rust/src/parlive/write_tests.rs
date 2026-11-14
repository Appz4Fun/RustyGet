use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

struct Disk(PathBuf);
impl Disk {
    fn new(tag: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rustyget-write-{tag}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::write(&path, b"original").unwrap();
        Self(path)
    }
    fn target(&self) -> WriterTarget {
        WriterTarget {
            file_id: 40,
            path: self.0.clone(),
        }
    }
}
impl Drop for Disk {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}
fn control(id: i32) -> ControlLease {
    reserve_control(id, 4096, 500 * MIB).unwrap()
}
fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::yield_now();
    }
    false
}

#[test]
fn live_repair_drains_cache_before_patching() {
    let disk = Disk::new("cache-drain");
    let gate = WriteCoordinator::new(SessionGeneration(1));
    let writer = gate.writer(&WriterIdentity::File(40)).unwrap();
    std::thread::scope(|scope| {
        // Drop before scope joins even if an assertion or flush fails.
        let writer = writer;
        let repair_gate = Arc::clone(&gate);
        let path = disk.0.clone();
        let target = disk.target();
        let repair = scope.spawn(move || {
            let lease = repair_gate
                .begin_repair(SessionGeneration(1), vec![target], control(-1900201))
                .unwrap();
            std::fs::write(path, b"repaired").unwrap();
            lease.publish(&[(40, 0)], || ()).unwrap();
        });
        assert!(
            wait_until(|| gate.status().pending_repair),
            "repair did not reach the writer barrier"
        );
        assert_eq!(gate.status().active_writers, 1);
        let items = [crate::artwritercore::ArticleWriterFlushItem {
            segment_offset: 0,
            segment_size: 8,
            segment_content: b"cache000",
            result_filename: None,
        }];
        let (articles, bytes) = crate::artwritercore::article_writer_flush_cache(
            &items,
            true,
            false,
            disk.0.to_str().unwrap().as_bytes(),
            || false,
            |_| {},
            |message| panic!("{}", String::from_utf8_lossy(message)),
        );
        assert_eq!((articles, bytes), (1, 8));
        drop(writer);
        repair.join().unwrap();
    });
    assert_eq!(std::fs::read(&disk.0).unwrap(), b"repaired");
}

#[test]
fn late_article_cannot_overwrite_verified_repair() {
    let disk = Disk::new("late-writer");
    let gate = WriteCoordinator::new(SessionGeneration(2));
    let lease = gate
        .begin_repair(SessionGeneration(2), vec![disk.target()], control(-1900202))
        .unwrap();
    std::fs::write(&disk.0, b"repaired").unwrap();
    lease.publish(&[(40, 0)], || ()).unwrap();
    if let Ok(_writer) = gate.writer(&WriterIdentity::File(40)) {
        std::fs::write(&disk.0, b"late-old").unwrap();
    }
    assert_eq!(std::fs::read(&disk.0).unwrap(), b"repaired");
    assert!(gate.writer(&WriterIdentity::Path(disk.0.clone())).is_err());
    assert!(
        gate.writer(&WriterIdentity::File(41)).is_ok(),
        "other source files may continue"
    );
}

#[test]
fn replaced_generation_cannot_release_hold() {
    let disk = Disk::new("replace");
    let gate = WriteCoordinator::new(SessionGeneration(3));
    let held = AtomicBool::new(true);
    let lease = gate
        .begin_repair(SessionGeneration(3), vec![disk.target()], control(-1900203))
        .unwrap();
    gate.advance_generation(SessionGeneration(4)).unwrap();
    assert!(lease
        .publish(&[(40, 0)], || held.store(false, Ordering::SeqCst))
        .is_err());
    assert!(held.load(Ordering::SeqCst));
    assert!(gate.writer(&WriterIdentity::File(40)).is_ok());
}

#[test]
fn cancelled_repair_keeps_hold() {
    let disk = Disk::new("cancel");
    let gate = WriteCoordinator::new(SessionGeneration(5));
    let held = AtomicBool::new(true);
    let lease = gate
        .begin_repair(SessionGeneration(5), vec![disk.target()], control(-1900204))
        .unwrap();
    gate.cancel_generation(SessionGeneration(5));
    assert!(lease
        .publish(&[(40, 0)], || held.store(false, Ordering::SeqCst))
        .is_err());
    assert!(held.load(Ordering::SeqCst));
    assert_eq!(std::fs::read(&disk.0).unwrap(), b"original");
}

#[test]
fn cleanup_aborts_pending_repair_and_rejects_late_io() {
    let disk = Disk::new("cleanup");
    let gate = WriteCoordinator::new(SessionGeneration(6));
    let writer = gate.writer(&WriterIdentity::File(40)).unwrap();
    std::thread::scope(|scope| {
        let writer = writer;
        let repair_gate = Arc::clone(&gate);
        let target = disk.target();
        let repair = scope.spawn(move || {
            repair_gate.begin_repair(SessionGeneration(6), vec![target], control(-1900206))
        });
        let reached = wait_until(|| gate.status().pending_repair);
        gate.close();
        drop(writer);
        assert!(reached);
        assert!(repair.join().unwrap().is_err());
    });
    assert!(gate.writer(&WriterIdentity::File(40)).is_err());
    assert_eq!(std::fs::read(&disk.0).unwrap(), b"original");
}

#[test]
fn certification_retains_control_reservation_until_invalidation() {
    let disk = Disk::new("cert-budget");
    let gate = WriteCoordinator::new(SessionGeneration(7));
    let lease = gate
        .begin_repair(
            SessionGeneration(7),
            vec![disk.target()],
            reserve_control(-1900207, MIB, 500 * MIB).unwrap(),
        )
        .unwrap();
    lease.publish(&[(40, 0)], || ()).unwrap();
    assert!(reserve_control(-1900207, 256, 500 * MIB).is_err());
    gate.advance_generation(SessionGeneration(8)).unwrap();
    assert!(reserve_control(-1900207, 256, 500 * MIB).is_ok());
}

#[test]
fn closed_writer_context_cannot_reopen_or_publish() {
    let disk = Disk::new("permanent-close");
    let gate = WriteCoordinator::new(SessionGeneration(9));
    let held = AtomicBool::new(true);
    let lease = gate
        .begin_repair(SessionGeneration(9), vec![disk.target()], control(-1900209))
        .unwrap();
    gate.close();
    assert!(gate.advance_generation(SessionGeneration(9)).is_err());
    assert!(lease
        .publish(&[(40, 0)], || held.store(false, Ordering::SeqCst))
        .is_err());
    assert!(held.load(Ordering::SeqCst));
    assert!(gate.writer(&WriterIdentity::File(40)).is_err());
}

#[test]
fn source_and_donor_writers_share_the_pre_metadata_barrier() {
    let disk = Disk::new("shared-root");
    let root = disk
        .0
        .parent()
        .unwrap()
        .join(format!("gate-root-{}", std::process::id()));
    let source = source_write_context(-1900210, 40, &root, b"part40.rar").unwrap();
    let donor = donor_write_context(&root, &root.join("part40.rar")).unwrap();
    let first = source.permit().unwrap();
    let second = donor.permit().unwrap();
    let gate = download_writes(-1900210, &root).unwrap();
    assert_eq!(
        gate.status().active_writers,
        2,
        "metadata startup must see both existing write paths"
    );
    drop(first);
    drop(second);
    gate.advance_generation(SessionGeneration(10)).unwrap();
    let target = WriterTarget {
        file_id: 40,
        path: root.join("part40.rar"),
    };
    gate.begin_repair(SessionGeneration(10), vec![target], control(-1900210))
        .unwrap()
        .publish(&[(40, 0)], || ())
        .unwrap();
    assert!(source.permit().is_err());
    assert!(donor.permit().is_err());
    forget_download_writes(-1900210);
}

#[test]
fn queue_side_writer_admission_never_waits_for_a_repair() {
    let disk = Disk::new("nonblocking");
    let gate = WriteCoordinator::new(SessionGeneration(11));
    let lease = gate
        .begin_repair(
            SessionGeneration(11),
            vec![disk.target()],
            control(-1900211),
        )
        .unwrap();
    assert!(matches!(
        gate.try_writer(&WriterIdentity::File(41)),
        Err(write_control::WriteBlocked::Busy)
    ));
    drop(lease);
    assert!(gate.try_writer(&WriterIdentity::File(41)).is_ok());
    gate.close();
    assert!(matches!(
        gate.try_writer(&WriterIdentity::File(41)),
        Err(write_control::WriteBlocked::Closed)
    ));
}

#[test]
fn live_file_mutations_cannot_remove_or_replace_certified_output() {
    let disk = Disk::new("file-mutation");
    let root = disk.0.parent().unwrap();
    let context = donor_write_context(root, &disk.0).unwrap();
    let gate = download_writes(-1900212, root).unwrap();
    gate.advance_generation(SessionGeneration(12)).unwrap();
    gate.begin_repair(
        SessionGeneration(12),
        vec![disk.target()],
        control(-1900212),
    )
    .unwrap()
    .publish(&[(40, 0)], || ())
    .unwrap();
    let replacement = disk.0.with_extension("replacement");
    std::fs::write(&replacement, b"late-old").unwrap();
    assert!(context
        .perform_io(|| std::fs::rename(&replacement, &disk.0))
        .is_err());
    assert!(context
        .perform_io(|| std::fs::remove_file(&disk.0))
        .is_err());
    assert_eq!(std::fs::read(&disk.0).unwrap(), b"original");
    assert_eq!(std::fs::read(&replacement).unwrap(), b"late-old");
    forget_download_writes(-1900212);
    std::fs::remove_file(replacement).unwrap();
}

#[test]
fn pre_metadata_writers_join_once_but_replaced_cohorts_cannot_write() {
    let disk = Disk::new("source-epoch");
    let root = disk
        .0
        .parent()
        .unwrap()
        .join(format!("epoch-root-{}", std::process::id()));
    let old = source_write_context(-1900213, 40, &root, b"part40.rar").unwrap();
    let gate = download_writes(-1900213, &root).unwrap();
    gate.advance_generation(SessionGeneration(13)).unwrap();
    assert!(
        old.permit().is_ok(),
        "first index must adopt writers already receiving bytes"
    );
    gate.advance_generation(SessionGeneration(14)).unwrap();
    assert!(
        matches!(
            old.permit(),
            Err(write_control::WriteBlocked::StaleGeneration)
        ),
        "old buffered writes survived session replacement"
    );
    let fresh = source_write_context(-1900213, 40, &root, b"part40.rar").unwrap();
    assert!(fresh.permit().is_ok());
    forget_download_writes(-1900213);
}

#[test]
fn verified_target_crc_is_scoped_to_current_generation_id_and_path() {
    let disk = Disk::new("certified-crc");
    let gate = WriteCoordinator::new(SessionGeneration(80));
    assert_eq!(gate.certified_crc(SessionGeneration(80), 40, &disk.0), None);
    let lease = gate
        .begin_repair(
            SessionGeneration(80),
            vec![disk.target()],
            control(-1900280),
        )
        .unwrap();
    std::fs::write(&disk.0, b"repaired").unwrap();
    lease
        .publish(&[(40, crc32fast::hash(b"repaired"))], || ())
        .unwrap();
    assert_eq!(
        gate.certified_crc(SessionGeneration(80), 40, &disk.0),
        Some(crc32fast::hash(b"repaired"))
    );
    assert_eq!(gate.certified_crc(SessionGeneration(80), 41, &disk.0), None);
    assert_eq!(
        gate.certified_crc(SessionGeneration(80), 40, &disk.0.with_extension("other")),
        None
    );
    assert_eq!(gate.certified_crc(SessionGeneration(81), 40, &disk.0), None);
    gate.cancel_generation(SessionGeneration(80));
    assert_eq!(gate.certified_crc(SessionGeneration(80), 40, &disk.0), None);
    gate.advance_generation(SessionGeneration(81)).unwrap();
    assert_eq!(gate.certified_crc(SessionGeneration(81), 40, &disk.0), None);
}
