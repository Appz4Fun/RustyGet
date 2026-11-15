use super::*;
use std::sync::atomic::{AtomicI32, Ordering};
static NEXT_NAMESPACE_JOB: AtomicI32 = AtomicI32::new(-1900400);

fn metadata(files: &[(&[u8], u8)]) -> PacketSet {
    use par_rs::packet::{compute_set_id, encode_main_body, FileDescPacket, MainPacket};
    let mut set = PacketSet::new();
    for &(name, hash) in files {
        let mut desc = FileDescPacket {
            file_id: FileId([0; 16]),
            hash_full: [hash.wrapping_add(1); 16],
            hash_16k: [hash; 16],
            file_length: 32768,
            name: name.to_vec(),
        };
        desc.file_id = desc.computed_file_id();
        assert!(desc.id_consistent());
        set.file_descs.insert(desc.file_id, desc);
    }
    let ids = set.file_descs.keys().copied().collect::<Vec<_>>();
    let body = encode_main_body(512, &ids, &[]);
    let id = compute_set_id(&body);
    set.set_id = Some(id);
    set.main = Some(MainPacket::parse(id, &body).unwrap());
    set
}
fn row(id: i32, name: &[u8], hash: &[u8]) -> NamespaceRow {
    NamespaceRow {
        queue_id: id,
        name: name.to_vec(),
        original: name.to_vec(),
        hash16k: hash.to_vec(),
        output: Vec::new(),
        par_file: false,
        deleted: false,
        completed: false,
    }
}
fn hash_hex(hash: u8) -> Vec<u8> {
    format!("{hash:02x}").repeat(16).into_bytes()
}
fn facts(rows: Vec<NamespaceRow>) -> NamespaceFacts {
    charged_facts(rows, 65536).1
}
fn charged_facts(rows: Vec<NamespaceRow>, bytes: u64) -> (i32, NamespaceFacts) {
    // Fixture input construction is not the native capture path; the latter
    // must size/reserve BEFORE copying any native row or validation scratch.
    let job = NEXT_NAMESPACE_JOB.fetch_sub(1, Ordering::Relaxed);
    let facts = NamespaceFacts {
        root: std::path::PathBuf::from("/established/téléchargements"),
        rows,
        reservation: crate::parlive::reserve_control(job, bytes, 500 << 20).unwrap(),
    };
    (job, facts)
}

#[test]
fn two_equal_names_among_larger_queue_are_ambiguous() {
    let set = metadata(&[(b"part40.rar", 1), (b"other.rar", 2)]);
    assert!(NamespaceProof::prepare(
        &set,
        facts(vec![
            row(40, b"part40.rar", b""),
            row(41, b"part40.rar", b""),
            row(42, b"other.rar", b"")
        ])
    )
    .is_err());
}
#[test]
fn physical_case_alias_is_rejected_before_any_payload_binding() {
    let set = metadata(&[(b"part40.rar", 1)]);
    assert!(NamespaceProof::prepare(
        &set,
        facts(vec![
            row(40, b"part40.rar", b""),
            row(41, b"PART40.RAR", b"")
        ])
    )
    .is_err());
}
#[test]
fn shared_unicode_root_and_distinct_ascii_leaves_are_eligible() {
    let set = metadata(&[(b"part40.rar", 1), (b"other.rar", 2)]);
    let mut first_row = row(40, b"part40.rar", b"");
    first_row.output = "/established/téléchargements/40.out.tmp"
        .as_bytes()
        .to_vec();
    let (job, captured) = charged_facts(vec![first_row, row(41, b"other.rar", b"")], 65536);
    let mut proof = NamespaceProof::prepare(&set, captured).unwrap();
    let first = set
        .file_descs
        .values()
        .find(|d| d.name == b"part40.rar")
        .unwrap()
        .file_id;
    assert_eq!(proof.bind(40, b"part40.rar", b"").unwrap(), Some(first));
    assert!(proof.bind(41, b"other.rar", b"").unwrap().is_some());
    let remainder = crate::parlive::reserve_control(job, (1 << 20) - 65536, 500 << 20).unwrap();
    assert!(crate::parlive::reserve_control(job, 256, 500 << 20).is_err());
    drop(proof);
    let released = crate::parlive::reserve_control(job, 65536, 500 << 20).unwrap();
    drop((released, remainder));
}
#[test]
fn uncertain_leaf_or_generated_output_collision_refuses_live_admission() {
    let set = metadata(&[(b"part40.rar", 1)]);
    for bad in [
        b"../part40.rar".as_slice(),
        "parté.rar".as_bytes(),
        b"part40.rar.",
        b"40.out.tmp",
        b"40.001",
        b"part\\40.rar",
    ] {
        assert!(
            NamespaceProof::prepare(
                &set,
                facts(vec![row(40, b"part40.rar", b""), row(41, bad, b"")])
            )
            .is_err(),
            "unprovable namespace must not install"
        );
    }
    let mut wrong_output = row(41, b"other.rar", b"");
    wrong_output.output = "/established/téléchargements/part40.rar"
        .as_bytes()
        .to_vec();
    assert!(
        NamespaceProof::prepare(&set, facts(vec![row(40, b"part40.rar", b""), wrong_output]))
            .is_err(),
        "logical names cannot hide an aliased physical output"
    );
    let (job, too_small) = charged_facts(vec![row(40, b"part40.rar", b"")], 256);
    assert!(NamespaceProof::prepare(&set, too_small).is_err());
    assert!(crate::parlive::reserve_control(job, 1 << 20, 500 << 20).is_ok());
}
#[test]
fn par_destination_case_aliases_do_not_resolve_by_first_match() {
    let set = metadata(&[(b"part40.rar", 1), (b"PART40.RAR", 2)]);
    assert!(NamespaceProof::prepare(&set, facts(vec![row(40, b"part40.rar", b"")])).is_err());
}
#[test]
fn unique_obfuscated_binding_waits_for_hash_then_is_monotonic() {
    let set = metadata(&[(b"part40.rar", 1)]);
    let fid = set.main.as_ref().unwrap().recoverable_file_ids[0];
    let mut proof =
        NamespaceProof::prepare(&set, facts(vec![row(40, b"obfuscated.bin", b"")])).unwrap();
    assert_eq!(proof.bind(40, b"obfuscated.bin", b"").unwrap(), None);
    assert_eq!(
        proof.bind(40, b"obfuscated.bin", &hash_hex(1)).unwrap(),
        Some(fid)
    );
    assert_eq!(
        proof.bind(40, b"obfuscated.bin", &hash_hex(1)).unwrap(),
        Some(fid)
    );
    assert!(proof.bind(40, b"renamed.bin", &hash_hex(1)).is_err());
    assert!(proof.bind(99, b"obfuscated.bin", &hash_hex(1)).is_err());
}
#[test]
fn second_queue_owner_cannot_lazily_bind_the_same_par_source() {
    let set = metadata(&[(b"part40.rar", 1)]);
    let mut proof = NamespaceProof::prepare(
        &set,
        facts(vec![
            row(40, b"obfuscated-a", b""),
            row(41, b"obfuscated-b", b""),
        ]),
    )
    .unwrap();
    assert!(proof
        .bind(40, b"obfuscated-a", &hash_hex(1))
        .unwrap()
        .is_some());
    assert!(proof.bind(41, b"obfuscated-b", &hash_hex(1)).is_err());
}
#[test]
fn ambiguous_hash_only_mapping_does_not_reject_unique_named_sources() {
    let set = metadata(&[(b"part40.rar", 1), (b"other.rar", 1)]);
    let mut proof = NamespaceProof::prepare(
        &set,
        facts(vec![
            row(40, b"part40.rar", b""),
            row(41, b"other.rar", b""),
            row(42, b"obfuscated.bin", b""),
        ]),
    )
    .unwrap();
    assert!(proof.bind(40, b"part40.rar", b"").unwrap().is_some());
    assert!(proof.bind(41, b"other.rar", b"").unwrap().is_some());
    assert!(proof.bind(42, b"obfuscated.bin", &hash_hex(1)).is_err());
}
#[test]
fn direct_rename_to_the_par_destination_keeps_the_namespace() {
    // Obfuscated posts: the direct renamer gives each queue row its real name
    // while live PAR runs. That must not invalidate the session.
    let set = metadata(&[(b"part40.rar", 1), (b"part41.rar", 2)]);
    let ids = set.main.as_ref().unwrap().recoverable_file_ids.clone();
    let mut par = row(50, b"x7Hq2.par2", b"");
    par.par_file = true;
    let mut proof = NamespaceProof::prepare(
        &set,
        facts(vec![
            row(40, b"ACZKzt_qrGtChr5.rar", b""),
            row(41, b"u2rGOjXAUK2eLrH58.rar", b""),
            par,
        ]),
    )
    .unwrap();
    // Renamed by name to its destination, then bound to it.
    let first = proof.bind(40, b"part40.rar", b"").unwrap().unwrap();
    assert!(ids.contains(&first));
    assert_eq!(proof.bind(40, b"part40.rar", b"").unwrap(), Some(first));
    assert!(proof.protects_leaf(b"PART40.RAR"));
    // The recovery row may be renamed too, through the recheck path.
    proof.begin_recheck();
    proof
        .recheck_row(40, 0, [b"part40.rar", b"", b"", b""])
        .unwrap();
    proof
        .recheck_row(41, 0, [b"part41.rar", b"", b"", b""])
        .unwrap();
    proof
        .recheck_row(50, 1, [b"release.vol00+01.par2", b"", b"", b""])
        .unwrap();
    proof.finish_recheck().unwrap();
}
#[test]
fn direct_rename_that_is_ambiguous_still_invalidates() {
    let set = metadata(&[(b"part40.rar", 1), (b"part41.rar", 2)]);
    // Renamed onto another row's name.
    let mut proof = NamespaceProof::prepare(
        &set,
        facts(vec![
            row(40, b"obf-a.rar", b""),
            row(41, b"part41.rar", b""),
        ]),
    )
    .unwrap();
    assert!(proof.bind(40, b"PART41.rar", b"").is_err());
    // A bound source renamed away from its PAR destination.
    let mut proof =
        NamespaceProof::prepare(&set, facts(vec![row(40, b"obf-a.rar", b"")])).unwrap();
    assert!(proof.bind(40, b"obf-a.rar", &hash_hex(1)).unwrap().is_some());
    assert!(proof.bind(40, b"movie.mkv", b"").is_err());
    // A known 16k hash that names a different destination.
    let mut proof =
        NamespaceProof::prepare(&set, facts(vec![row(40, b"obf-a.rar", b"")])).unwrap();
    assert!(proof.bind(40, b"part41.rar", &hash_hex(1)).is_err());
    // Renamed onto the destination another row is already bound to.
    let mut proof = NamespaceProof::prepare(
        &set,
        facts(vec![
            row(40, b"obf-a.rar", b""),
            row(41, b"obf-b.rar", b""),
        ]),
    )
    .unwrap();
    assert!(proof.bind(40, b"obf-a.rar", &hash_hex(2)).unwrap().is_some());
    assert!(proof.bind(41, b"part41.rar", b"").is_err());
    // A recovery row renamed onto a source destination.
    let mut par = row(50, b"x7Hq2.par2", b"");
    par.par_file = true;
    let mut proof = NamespaceProof::prepare(&set, facts(vec![par])).unwrap();
    proof.begin_recheck();
    assert!(proof
        .recheck_row(50, 1, [b"part40.rar", b"", b"", b""])
        .is_err());
}
#[test]
fn unbound_source_renamed_into_another_par_set_stays_unbound() {
    // Jobs with two PAR sets: the renamer names set-B files from set B.
    let set = metadata(&[(b"part40.rar", 1)]);
    let mut proof = NamespaceProof::prepare(
        &set,
        facts(vec![row(40, b"obf-a.rar", b""), row(41, b"obf-b.rar", b"")]),
    )
    .unwrap();
    assert_eq!(proof.bind(41, b"other-set.part01.rar", b"").unwrap(), None);
    assert!(proof.bind(40, b"part40.rar", b"").unwrap().is_some());
}
#[test]
fn rename_to_a_duplicated_16k_hash_does_not_bind_by_name() {
    let set = metadata(&[(b"part40.rar", 1), (b"part41.rar", 1)]);
    let mut proof = NamespaceProof::prepare(&set, facts(vec![row(40, b"obf-a.rar", b"")])).unwrap();
    assert!(proof.bind(40, b"part41.rar", &hash_hex(1)).is_err());
}
#[test]
fn renames_grow_the_control_reservation_on_demand() {
    let long = format!("{}.part40.rar", "r".repeat(240));
    let set = metadata(&[(long.as_bytes(), 1)]);
    // The smallest charge the captured namespace fits in, then a rename
    // that needs more than its slack.
    let mut jobs = Vec::new();
    let mut proof = None;
    for bytes in (256..65536).step_by(16) {
        let (job, captured) = charged_facts(vec![row(40, b"o.rar", b"")], bytes);
        jobs.push(job);
        if let Ok(prepared) = NamespaceProof::prepare(&set, captured) {
            proof = Some(prepared);
            break;
        }
    }
    let mut proof = proof.unwrap();
    assert!(proof.bind(40, long.as_bytes(), b"").unwrap().is_some());
    // Everything charged returns when the proof drops.
    drop(proof);
    for id in jobs {
        drop(crate::parlive::reserve_control(id, 1 << 20, 500 << 20).unwrap());
    }
}
#[test]
fn rename_by_article_name_survives_a_hash_matching_no_destination() {
    // A damaged or out-of-order first article gives a 16k hash no PAR entry
    // has; the article's own name still binds the row (job 210, 2026-11-15).
    let set = metadata(&[(b"part40.rar", 1), (b"part41.rar", 2)]);
    let mut proof = NamespaceProof::prepare(&set, facts(vec![row(40, b"obf-a.rar", b"")])).unwrap();
    assert!(proof.bind(40, b"part40.rar", &hash_hex(9)).unwrap().is_some());
}
