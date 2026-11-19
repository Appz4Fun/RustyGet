use super::*;
use crate::queuesubsys_host::{NzbRef, QueueSubsysHost};
use std::ffi::{c_char, c_int, c_void};

type Visitor = unsafe extern "C" fn(
    *mut c_void,
    c_int,
    u32,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
) -> bool;

struct Producer {
    calls: usize,
    status: usize,
    changed: bool,
}

unsafe extern "C" fn capture_host(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    _: c_int,
    _: usize,
    p2: usize,
    s1: *const c_char,
    _: *const c_char,
    _: *mut c_int,
    _: *mut i64,
    _: *mut *const c_char,
) -> usize {
    if op != 43 || a1 != 15 {
        return 0;
    }
    let producer = &mut *ctx.cast::<Producer>();
    producer.calls += 1;
    if producer.status != 1 {
        return producer.status;
    }
    let visitor: Visitor = std::mem::transmute(s1);
    let name = if producer.changed && producer.calls == 2 {
        c"changed-name.bin"
    } else {
        c"part01.rar"
    };
    usize::from(visitor(
        p2 as *mut c_void,
        1,
        0,
        name.as_ptr(),
        c"original".as_ptr(),
        c"".as_ptr(),
        c"".as_ptr(),
    ))
}

fn capture(
    producer: &mut Producer,
    job: i32,
) -> Result<live_namespace::NamespaceFacts, RepairError> {
    let host = QueueSubsysHost::new_mock(std::ptr::from_mut(producer).cast(), capture_host);
    let root = Path::new("/capture-root");
    host.live_namespace_capture(NzbRef::from_addr(1), job, root, 8192, 500 << 20)
}

#[test]
fn live_capture_reserves_before_owned_copy() {
    let job = -1900701;
    let held = reserve_control(job, (1 << 20) - 16, 500 << 20).unwrap();
    let mut producer = Producer {
        calls: 0,
        status: 1,
        changed: false,
    };
    assert!(capture(&mut producer, job).is_err());
    assert_eq!(producer.calls, 1, "quota denial must precede copying pass");
    drop(held);
    let facts = capture(&mut producer, job).unwrap();
    assert_eq!(facts.rows.len(), 1);
    assert_eq!(facts.rows[0].name, b"part01.rar");
    assert_eq!(facts.rows[0].original, b"original");
    let remaining = reserve_control(job, (1 << 20) - facts.reservation.bytes(), 500 << 20).unwrap();
    assert!(reserve_control(job, 256, 500 << 20).is_err());
    drop(facts);
    assert!(reserve_control(job, 256, 500 << 20).is_ok());
    drop(remaining);
}

#[test]
fn live_capture_busy_refusal_and_protocol_status_are_distinct() {
    for status in [0, 2, 9] {
        let mut producer = Producer {
            calls: 0,
            status,
            changed: false,
        };
        let error = capture(&mut producer, -1900702).err().unwrap();
        assert_eq!(streaming_session_capture_busy(&error), status == 2);
        assert_eq!(producer.calls, 1);
    }
    assert!(reserve_control(-1900702, 1 << 20, 500 << 20).is_ok());
}

#[test]
fn live_capture_changed_second_pass_refuses_and_refunds() {
    let mut producer = Producer {
        calls: 0,
        status: 1,
        changed: true,
    };
    assert!(capture(&mut producer, -1900703).is_err());
    assert_eq!(producer.calls, 2);
    assert!(reserve_control(-1900703, 1 << 20, 500 << 20).is_ok());
}

#[test]
fn live_capture_consistent_queued_completed_handoff_is_one_owner() {
    use live_namespace::{NamespaceFacts, NamespaceProof, NamespaceRow};
    let set = load_bounded_live_metadata(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/testdata/parchecker/testfile.par2"),
    )
    .unwrap();
    let desc = set.file_descs.values().next().unwrap();
    let row = |completed| NamespaceRow {
        queue_id: 1,
        name: desc.name.clone(),
        original: desc.name.clone(),
        hash16k: Vec::new(),
        output: Vec::new(),
        par_file: false,
        deleted: false,
        completed,
    };
    let facts = NamespaceFacts {
        root: PathBuf::from("/capture-root"),
        rows: vec![row(true), row(false)],
        reservation: reserve_control(-1900704, 16384, 500 << 20).unwrap(),
    };
    assert!(facts.rows[0].completed);
    let mut proof = NamespaceProof::prepare(&set, facts).unwrap();
    assert_eq!(proof.bind(1, &desc.name, b"").unwrap(), Some(desc.file_id));
    proof.begin_recheck();
    proof
        .recheck_row(1, 4, [&desc.name, &desc.name, b"", b""])
        .unwrap();
    proof.finish_recheck().unwrap();
    proof.begin_recheck();
    assert!(proof
        .recheck_row(2, 4, [&desc.name, &desc.name, b"", b""])
        .is_err());
    assert!(proof.finish_recheck().is_err());
}

fn fixture_facts(job: i32, root: &Path, set: &PacketSet) -> live_namespace::NamespaceFacts {
    live_namespace::NamespaceFacts {
        root: root.to_path_buf(),
        rows: set
            .main
            .as_ref()
            .unwrap()
            .recoverable_file_ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let desc = &set.file_descs[id];
                live_namespace::NamespaceRow {
                    queue_id: i as i32 + 1,
                    name: desc.name.clone(),
                    original: desc.name.clone(),
                    hash16k: Vec::new(),
                    output: Vec::new(),
                    par_file: false,
                    deleted: false,
                    completed: false,
                }
            })
            .collect(),
        reservation: reserve_control(job, 16384, 500 << 20).unwrap(),
    }
}

#[test]
fn live_capture_bound_queue_id_precedes_any_snapshot() {
    let fx = super::live_lifecycle_tests::Fixture::new();
    let set = fx.set();
    let job = -1900705;
    let proof =
        live_namespace::NamespaceProof::prepare(&set, fixture_facts(job, &fx.0, &set)).unwrap();
    let writers = WriteCoordinator::new(SessionGeneration(0));
    writers
        .bind_namespace_generation(SessionGeneration(1), proof, || ())
        .unwrap();
    let desc = &set.file_descs[&set.main.as_ref().unwrap().recoverable_file_ids[0]];
    assert_eq!(
        writers
            .namespace_source_id(SessionGeneration(1), Some(1), &desc.name, b"")
            .unwrap(),
        Some(Some(desc.file_id))
    );
    assert!(writers
        .namespace_source_id(SessionGeneration(0), Some(1), &desc.name, b"")
        .is_err());
    assert!(
        writers.is_current_generation(SessionGeneration(1)),
        "stale context poisoned current proof"
    );
    assert!(writers
        .namespace_source_id(SessionGeneration(1), Some(999), &desc.name, b"")
        .is_err());
    assert!(
        !writers.is_current_generation(SessionGeneration(1)),
        "unknown owner did not latch fallback"
    );
}

#[test]
fn live_capture_prepared_session_stays_unpublished_on_busy_recheck() {
    let fx = super::live_lifecycle_tests::Fixture::new();
    let set = fx.set();
    let job = -1900706;
    let writers = download_writes(job, &fx.0).unwrap();
    let metadata =
        prepare_live_start(&fx.0.join("testfile.par2"), &fx.0, 500 << 20, writers).unwrap();
    assert!(metadata.catalog_required_bytes().unwrap() < 16384);
    let mut prepared = metadata
        .finish(&fx.0.join("testfile.par2"), fixture_facts(job, &fx.0, &set))
        .unwrap();
    assert!(active_streaming_session(job).is_none());
    let error = prepared
        .check_and_install(job, |_| {
            Err(RepairError::Io {
                context: "live namespace busy".into(),
                source: std::io::ErrorKind::WouldBlock.into(),
            })
        })
        .err()
        .unwrap();
    assert!(streaming_session_capture_busy(&error));
    assert!(active_streaming_session(job).is_none());
    drop(prepared);
    assert!(reserve_control(job, 1 << 20, 500 << 20).is_ok());
    crate::parlive::forget_download_writes(job);
}

struct LockedProducer {
    locked: std::cell::Cell<bool>,
    calls: std::cell::Cell<usize>,
    busy_on: std::cell::Cell<usize>,
    rows: Vec<std::ffi::CString>,
}

unsafe extern "C" fn locked_capture_host(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    _: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    _: *const c_char,
    _: *mut c_int,
    _: *mut i64,
    _: *mut *const c_char,
) -> usize {
    let producer = &*ctx.cast::<LockedProducer>();
    if op == 20 && a1 == 0 {
        if producer.locked.replace(true) {
            return 0;
        }
        let callback: unsafe extern "C" fn(*mut c_void, usize) = std::mem::transmute(p1);
        callback(p2 as *mut c_void, 1);
        producer.locked.set(false);
        return 1;
    }
    if op != 43 || a1 != 15 || !producer.locked.get() {
        return 0;
    }
    let pass = producer.calls.get() + 1;
    producer.calls.set(pass);
    if pass == producer.busy_on.get() {
        return 2;
    }
    let visitor: Visitor = std::mem::transmute(s1);
    for (i, name) in producer.rows.iter().enumerate().rev() {
        // Final recheck may observe the consistent queued-to-completed handoff.
        let flags = if pass % 3 == 0 { 4 } else { 0 };
        if !visitor(
            p2 as *mut c_void,
            i as i32 + 1,
            flags,
            name.as_ptr(),
            name.as_ptr(),
            c"".as_ptr(),
            c"".as_ptr(),
        ) {
            return 0;
        }
    }
    1
}

#[test]
fn live_capture_final_busy_retries_real_preparation_then_gates_ram_copy() {
    let fx = super::live_lifecycle_tests::Fixture::new();
    let set = fx.set();
    let job = -1900707;
    let fence = LiveStartupFence::default();
    let producer = LockedProducer {
        locked: false.into(),
        calls: 0.into(),
        busy_on: 3.into(),
        rows: set
            .main
            .as_ref()
            .unwrap()
            .recoverable_file_ids
            .iter()
            .map(|id| std::ffi::CString::new(set.file_descs[id].name.clone()).unwrap())
            .collect(),
    };
    let host = QueueSubsysHost::new_mock(
        std::ptr::from_ref(&producer).cast_mut().cast(),
        locked_capture_host,
    );
    streaming_session_queue_completed_file(job, 4096, || {
        Ok(LiveCompletedFile {
            nzb_id: job,
            base_dir: fx.0.clone(),
            path: fx.0.join("testfile.par2"),
            name: b"testfile.par2".to_vec(),
            hash16k: Vec::new(),
            par_file: true,
            successful: true,
            accepted: Vec::new(),
            queued_id: Some(100),
        })
    })
    .unwrap();
    let start = |file: &LiveCompletedFile, writers: Arc<WriteCoordinator>| {
        assert!(!producer.locked.get());
        let attempt = fence.begin()?;
        let metadata = prepare_live_start(&file.path, &file.base_dir, 500 << 20, writers)?;
        let catalog = metadata.catalog_required_bytes()?;
        let facts = host.with_locked_queue(|_| {
            let nzb = NzbRef::from_addr(1);
            host.live_namespace_capture(nzb, job, &file.base_dir, catalog, 500 << 20)
        })?;
        assert!(!producer.locked.get());
        let mut prepared = metadata.finish_tracked(&file.path, facts, &attempt)?;
        assert!(active_streaming_session(job).is_none());
        let installed = host.with_locked_queue(|_| {
            prepared.check_and_install(job, |proof| {
                host.live_namespace_visit(NzbRef::from_addr(1), |row| {
                    proof.recheck_row(row.queue_id, row.flags, row.fields)
                })
            })
        })?;
        assert!(!producer.locked.get());
        installed.finish(job);
        Ok(())
    };
    let error = streaming_session_process_completed_file_with_start(job, |_| Ok(()), start)
        .unwrap()
        .err()
        .unwrap();
    assert!(streaming_session_capture_busy(&error));
    assert_eq!(producer.calls.get(), 3);
    assert!(active_streaming_session(job).is_none());
    let remainder = reserve_control(job, (1 << 20) - 4096, 500 << 20).unwrap();
    assert!(reserve_control(job, 256, 500 << 20).is_err());
    drop(remainder);
    assert!(streaming_session_process_completed_file_with_start(
        job,
        |_| panic!("busy event retried before tick"),
        start
    )
    .is_none());
    producer.busy_on.set(0);
    streaming_session_retry_completion_tick();
    assert!(
        streaming_session_process_completed_file_with_start(job, |_| Ok(()), start)
            .unwrap()
            .unwrap()
    );
    assert_eq!(producer.calls.get(), 6);
    let desc = &set.file_descs[&set.main.as_ref().unwrap().recoverable_file_ids[0]];
    let snapshots = std::cell::Cell::new(0);
    assert!(streaming_session_defer_article_for_queue(
        job,
        (Some(999), &desc.name, b""),
        0,
        4,
        0,
        || {
            snapshots.set(snapshots.get() + 1);
            Some(vec![0; 4])
        }
    )
    .unwrap()
    .is_err());
    assert_eq!(
        snapshots.get(),
        0,
        "unknown owner reached the RAM-copy closure"
    );
    assert!(active_streaming_session(job)
        .unwrap()
        .session
        .is_cancelled());
    streaming_session_forget(job);
    assert!(reserve_control(job, 1 << 20, 500 << 20).is_ok());
}

#[test]
fn live_capture_current_policy_root_and_ordinary_takeover_are_required() {
    use crate::queuesubsys_host::NzbSnap;
    use crate::status::PostStage;
    let root = Path::new("/capture-root");
    let mut current = NzbSnap {
        dest_dir: b"/capture-root".to_vec(),
        ..Default::default()
    };
    let matches = |nzb: &NzbSnap, automatic| {
        crate::queuecoord::live_start_context_matches(nzb, root, automatic)
    };
    assert!(matches(&current, true));
    assert!(
        !matches(&current, false),
        "manual/repair-off admitted startup"
    );
    current.dest_dir = b"/changed-root".to_vec();
    assert!(!matches(&current, true));
    current.dest_dir = b"/capture-root".to_vec();
    current.deleting = true;
    assert!(!matches(&current, true));
    current.deleting = false;
    current.many_dupe_files = true;
    assert!(!matches(&current, true));
    current.many_dupe_files = false;
    current.has_post_info = true;
    current.post_stage = PostStage::Queued;
    assert!(matches(&current, true));
    current.post_working = true;
    assert!(!matches(&current, true));
    current.post_working = false;
    current.post_stage = PostStage::LoadingPars;
    assert!(!matches(&current, true));
}

#[test]
fn live_capture_stop_cancels_unpublished_engine_and_fences_restart() {
    let fx = super::live_lifecycle_tests::Fixture::new();
    let set = fx.set();
    let job = -1900708;
    let fence = LiveStartupFence::default();
    let attempt = fence.begin().unwrap();
    let metadata = prepare_live_start(
        &fx.0.join("testfile.par2"),
        &fx.0,
        500 << 20,
        download_writes(job, &fx.0).unwrap(),
    )
    .unwrap();
    let mut prepared = metadata
        .finish_tracked(
            &fx.0.join("testfile.par2"),
            fixture_facts(job, &fx.0, &set),
            &attempt,
        )
        .unwrap();
    assert!(active_streaming_session(job).is_none());
    fence.stop(|| ());
    assert!(
        prepared.entry.session.is_cancelled(),
        "stop missed the unpublished engine"
    );
    assert!(matches!(
        prepared
            .entry
            .session
            .on_recovery_volume_arrived(&fx.0.join("never-opened.par2"), None),
        Err(RepairError::Cancelled)
    ));
    assert!(fence.reset());
    let check = |proof: &mut live_namespace::NamespaceProof| {
        for (i, id) in set
            .main
            .as_ref()
            .unwrap()
            .recoverable_file_ids
            .iter()
            .enumerate()
        {
            let name = &set.file_descs[id].name;
            proof.recheck_row(i as i32 + 1, 0, [name, name, b"", b""])?;
        }
        Ok(())
    };
    assert!(
        prepared.check_and_install(job, check).is_err(),
        "old preparation survived stop/restart"
    );
    assert!(active_streaming_session(job).is_none());
    let remainder = reserve_control(job, (1 << 20) - 16384, 500 << 20).unwrap();
    assert!(
        reserve_control(job, 256, 500 << 20).is_err(),
        "cancel released metadata still owned by the candidate"
    );
    drop(prepared);
    assert!(reserve_control(job, 16384, 500 << 20).is_ok());
    drop(remainder);
    crate::parlive::forget_download_writes(job);
}

#[test]
fn live_capture_three_same_id_roles_always_refuse() {
    let fx = super::live_lifecycle_tests::Fixture::new();
    let set = fx.set();
    let desc = &set.file_descs[&set.main.as_ref().unwrap().recoverable_file_ids[0]];
    for roles in [
        [false, true, false],
        [true, false, false],
        [false, false, true],
        [true, false, true],
        [false, true, true],
        [true, true, false],
    ] {
        let facts = live_namespace::NamespaceFacts {
            root: fx.0.clone(),
            rows: roles
                .iter()
                .map(|completed| live_namespace::NamespaceRow {
                    queue_id: 1,
                    name: desc.name.clone(),
                    original: desc.name.clone(),
                    hash16k: Vec::new(),
                    output: Vec::new(),
                    par_file: false,
                    deleted: false,
                    completed: *completed,
                })
                .collect(),
            reservation: reserve_control(-1900709, 16384, 500 << 20).unwrap(),
        };
        assert!(
            live_namespace::NamespaceProof::prepare(&set, facts).is_err(),
            "accepted duplicate roles {roles:?}"
        );
    }
}

#[test]
fn live_capture_metadata_preparation_cannot_cross_stop_restart() {
    let fx = super::live_lifecycle_tests::Fixture::new();
    let set = fx.set();
    let job = -1900710;
    let fence = LiveStartupFence::default();
    let attempt = fence.begin().unwrap();
    let metadata = prepare_live_start(
        &fx.0.join("testfile.par2"),
        &fx.0,
        500 << 20,
        download_writes(job, &fx.0).unwrap(),
    )
    .unwrap();
    fence.stop(|| ());
    assert!(fence.reset());
    assert!(
        metadata
            .finish_tracked(
                &fx.0.join("testfile.par2"),
                fixture_facts(job, &fx.0, &set),
                &attempt
            )
            .is_err(),
        "old metadata preparation crossed stop/restart before engine registration"
    );
    crate::parlive::forget_download_writes(job);
}
