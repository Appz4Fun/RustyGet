use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static NEXT_DISK: AtomicU64 = AtomicU64::new(1);

struct Disk(PathBuf);
impl Disk {
    fn new(tag: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rustyget-write-{tag}-{}-{nonce}-{}",
            std::process::id(),
            NEXT_DISK.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("source.rar");
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
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
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
fn certification_retains_control_reservation_until_cohort_close() {
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
    assert!(reserve_control(-1900207, 256, 500 * MIB).is_err());
    gate.close();
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

#[test]
fn source_conflict_before_metadata_cannot_be_erased_by_startup() {
    let disk = Disk::new("source-conflict-before-index");
    let root = disk.0.with_extension("cohort");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("part40.rar");
    std::fs::write(&path, b"original").unwrap();
    let context = source_write_context(-1900214, 40, &root, b"part40.rar").unwrap();
    let gate = download_writes(-1900214, &root).unwrap();
    context.invalidate_identity();
    assert!(
        gate.advance_generation(SessionGeneration(1)).is_err(),
        "first metadata erased conflicting pre-index source history"
    );
    context
        .perform_io(|| std::fs::write(&path, b"fallback"))
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"fallback");
    assert!(gate.advance_generation(SessionGeneration(2)).is_err());
    forget_download_writes(-1900214);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn stale_source_identity_attempt_cannot_poison_a_newer_cohort() {
    let disk = Disk::new("stale-source-identity");
    let root = disk.0.with_extension("cohort");
    let old = source_write_context(-1900215, 40, &root, b"part40.rar").unwrap();
    let gate = download_writes(-1900215, &root).unwrap();
    gate.advance_generation(SessionGeneration(1)).unwrap();
    gate.advance_generation(SessionGeneration(2)).unwrap();
    assert!(gate.is_current_generation(SessionGeneration(2)));
    old.invalidate_identity();
    assert!(
        gate.is_current_generation(SessionGeneration(2)),
        "a refused old context poisoned the current source cohort"
    );
    forget_download_writes(-1900215);
}

#[test]
fn refused_source_identity_attempt_keeps_the_target_certificate() {
    let disk = Disk::new("sealed-source-identity");
    let root = disk.0.parent().unwrap();
    let context = source_write_context(
        -1900216,
        40,
        root,
        disk.0.file_name().unwrap().as_encoded_bytes(),
    )
    .unwrap();
    let gate = download_writes(-1900216, root).unwrap();
    gate.advance_generation(SessionGeneration(1)).unwrap();
    gate.begin_repair(SessionGeneration(1), vec![disk.target()], control(-1900216))
        .unwrap()
        .publish(&[(40, 123)], || ())
        .unwrap();
    context.invalidate_identity();
    assert!(
        gate.is_current_generation(SessionGeneration(1)),
        "a certified target's refused replacement invalidated its certificate"
    );
    assert_eq!(
        gate.certified_crc(SessionGeneration(1), 40, &disk.0),
        Some(123)
    );
    assert!(context
        .perform_io(|| std::fs::write(&disk.0, b"wrong"))
        .is_err());
    assert_eq!(std::fs::read(&disk.0).unwrap(), b"original");
    forget_download_writes(-1900216);
}

#[test]
fn certified_target_seal_covers_ascii_case_alias_after_cancellation() {
    let disk = Disk::new("sealed-case-alias");
    let gate = WriteCoordinator::new(SessionGeneration(130));
    let lease = gate
        .begin_repair(
            SessionGeneration(130),
            vec![disk.target()],
            control(-1900330),
        )
        .unwrap();
    std::fs::write(&disk.0, b"repaired").unwrap();
    lease.publish(&[(40, 0)], || ()).unwrap();
    gate.cancel_generation(SessionGeneration(130));
    let alias = disk.0.with_file_name("SOURCE.RAR");
    let result = gate.writer(&WriterIdentity::FileAndPath(41, alias));
    assert!(
        matches!(result, Err(WriteBlocked::Certified)),
        "a second queue ID must not bypass a released target's path seal"
    );
    assert_eq!(std::fs::read(&disk.0).unwrap(), b"repaired");
}

#[test]
fn certified_target_seal_follows_the_exact_file() {
    let disk = Disk::new("sealed-uncertain-alias");
    let gate = WriteCoordinator::new(SessionGeneration(131));
    // Unsupported live identities keep their ordinary write path before any seal.
    assert!(gate
        .writer(&WriterIdentity::FileAndPath(
            41,
            disk.0.with_file_name("sourc\u{e9}.rar")
        ))
        .is_ok());
    let lease = gate
        .begin_repair(
            SessionGeneration(131),
            vec![disk.target()],
            control(-1900331),
        )
        .unwrap();
    lease.publish(&[(40, 0)], || ()).unwrap();
    // A path through another folder cannot be proven disjoint: refused.
    let up = WriterIdentity::FileAndPath(41, disk.0.parent().unwrap().join("sub/../source.rar"));
    assert!(matches!(gate.writer(&up), Err(WriteBlocked::Certified)));
    // A non-ASCII name that is the certified file (a hard link, as case folding or Unicode
    // normalization would give) is refused.
    let link = disk.0.with_file_name("sourc\u{e9}-link.rar");
    std::fs::hard_link(&disk.0, &link).unwrap();
    let identity = WriterIdentity::FileAndPath(41, link.clone());
    assert!(
        matches!(gate.writer(&identity), Err(WriteBlocked::Certified)),
        "the certified file under another name must not bypass the seal"
    );
    // Names that are other files (or no file yet) keep downloading: refusing them dropped
    // their cached articles as "already certified".
    for leaf in ["sourc\u{e9}.rar", "source.rar.", "other.rar"] {
        let identity = WriterIdentity::FileAndPath(42, disk.0.parent().unwrap().join(leaf));
        assert!(gate.writer(&identity).is_ok(), "{leaf} is another file");
    }
    let _ = std::fs::remove_file(link);
}

#[test]
fn established_unicode_root_keeps_flat_ascii_identity_provable() {
    let disk = Disk::new("unicode-root");
    let root = disk.0.parent().unwrap().join("téléchargements");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("source.rar");
    std::fs::write(&path, b"original").unwrap();
    let gate = WriteCoordinator::new(SessionGeneration(132));
    let lease = gate
        .begin_repair(
            SessionGeneration(132),
            vec![WriterTarget {
                file_id: 40,
                path: path.clone(),
            }],
            control(-1900332),
        )
        .unwrap();
    lease.publish(&[(40, 0)], || ()).unwrap();
    assert!(matches!(
        gate.writer(&WriterIdentity::FileAndPath(41, root.join("SOURCE.RAR"))),
        Err(WriteBlocked::Certified)
    ));
    assert!(gate
        .writer(&WriterIdentity::FileAndPath(42, root.join("other.rar")))
        .is_ok());
}

#[test]
fn replacing_live_generation_does_not_reopen_released_target() {
    let disk = Disk::new("sealed-generation");
    let gate = WriteCoordinator::new(SessionGeneration(133));
    let lease = gate
        .begin_repair(
            SessionGeneration(133),
            vec![disk.target()],
            control(-1900333),
        )
        .unwrap();
    std::fs::write(&disk.0, b"repaired").unwrap();
    lease.publish(&[(40, 0)], || ()).unwrap();
    gate.advance_generation(SessionGeneration(134)).unwrap();
    assert!(
        matches!(
            gate.writer(&WriterIdentity::FileAndPath(40, disk.0.clone())),
            Err(WriteBlocked::Certified)
        ),
        "generation replacement must retain the unpack-visible target seal"
    );
    assert_eq!(std::fs::read(&disk.0).unwrap(), b"repaired");
    assert!(
        matches!(
            gate.begin_repair(
                SessionGeneration(134),
                vec![disk.target()],
                control(-1900334)
            ),
            Err(WriteBlocked::Certified)
        ),
        "replacement generation must not bypass the seal through a second repair"
    );
}

#[test]
fn one_live_repair_request_cannot_claim_aliased_paths() {
    let disk = Disk::new("request-aliases");
    let gate = WriteCoordinator::new(SessionGeneration(135));
    let targets = vec![
        disk.target(),
        WriterTarget {
            file_id: 41,
            path: disk.0.with_file_name("SOURCE.RAR"),
        },
    ];
    assert!(
        matches!(
            gate.begin_repair(SessionGeneration(135), targets, control(-1900335)),
            Err(WriteBlocked::InvalidTarget)
        ),
        "one request cannot give two queue IDs the same potential target"
    );
    assert_eq!(std::fs::read(&disk.0).unwrap(), b"original");
    // Refusal leaves the gate usable for distinct targets in this same root.
    let targets = vec![
        disk.target(),
        WriterTarget {
            file_id: 41,
            path: disk.0.with_file_name("other.rar"),
        },
    ];
    let lease = gate
        .begin_repair(SessionGeneration(135), targets, control(-1900336))
        .expect("distinct eligible target names remain admissible");
    drop(lease);
}

#[test]
fn a_write_that_found_nothing_leaves_live_par_sources_valid() {
    for (id, error, survives) in [
        (-1900297, std::io::ErrorKind::NotFound, true),
        (-1900298, std::io::ErrorKind::Other, false),
    ] {
        let disk = Disk::new(&format!("found-nothing{id}"));
        let root = disk.0.parent().unwrap();
        let context = donor_write_context(root, &disk.0).unwrap();
        let gate = download_writes(id, root).unwrap();
        gate.advance_generation(SessionGeneration(1)).unwrap();
        let _ = context.perform_io(|| -> std::io::Result<()> { Err(error.into()) });
        assert_eq!(
            gate.is_current_generation(SessionGeneration(1)),
            survives,
            "{error:?} should leave live PAR running: {survives}"
        );
        forget_download_writes(id);
    }
}

/// Two missing non-ASCII names one repair request claims may be one file on a case- or
/// normalization-insensitive file system: still refused (only the certified check decides by
/// the file itself).
#[test]
fn one_repair_request_cannot_claim_two_missing_non_ascii_names() {
    let disk = Disk::new("request-nonascii");
    let gate = WriteCoordinator::new(SessionGeneration(137));
    let targets = vec![
        WriterTarget {
            file_id: 40,
            path: disk.0.with_file_name("\u{c4}rger.rar"),
        },
        WriterTarget {
            file_id: 41,
            path: disk.0.with_file_name("\u{e4}rger.rar"),
        },
    ];
    assert!(matches!(
        gate.begin_repair(SessionGeneration(137), targets, control(-1900337)),
        Err(WriteBlocked::InvalidTarget)
    ));
}

/// The seal follows the certified file, not its name: after a rename (par-rename gives an
/// obfuscated target its par2 name) and through a symlink, a write to it is still refused.
#[test]
fn certified_seal_follows_a_renamed_file_and_symlinks() {
    let disk = Disk::new("sealed-renamed");
    let gate = WriteCoordinator::new(SessionGeneration(138));
    let lease = gate
        .begin_repair(
            SessionGeneration(138),
            vec![disk.target()],
            control(-1900338),
        )
        .unwrap();
    lease.publish(&[(40, 0)], || ()).unwrap();
    let renamed = disk.0.with_file_name("\u{c4}rger.rar");
    std::fs::rename(&disk.0, &renamed).unwrap();
    assert!(matches!(
        gate.writer(&WriterIdentity::Path(renamed.clone())),
        Err(WriteBlocked::Certified)
    ));
    let link = disk.0.with_file_name("\u{c4}rger-sym.rar");
    std::os::unix::fs::symlink(&renamed, &link).unwrap();
    assert!(matches!(
        gate.writer(&WriterIdentity::Path(link.clone())),
        Err(WriteBlocked::Certified)
    ));
    // Another existing non-ASCII file in the folder is not the certified one.
    let other = disk.0.with_file_name("\u{d6}rger.rar");
    std::fs::write(&other, b"other").unwrap();
    assert!(gate
        .writer(&WriterIdentity::FileAndPath(42, other.clone()))
        .is_ok());
    let _ = std::fs::remove_file(link);
    let _ = std::fs::remove_file(other);
    let _ = std::fs::rename(&renamed, &disk.0);
}
