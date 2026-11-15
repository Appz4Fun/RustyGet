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
