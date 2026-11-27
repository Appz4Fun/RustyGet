#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&sel, rest)) = data.split_first() else { return };
    if rest.len() >= 8 {
        let _ = nzbget_rs::unpack::detect_rar_signature(&rest[..8]);
    }
    let _ = nzbget_rs::fuzz_entry::rar_read(rest, if sel & 1 != 0 { Some(b"pw") } else { None });
    let mid = rest.len() / 2;
    nzbget_rs::fuzz_entry::rar_twin_probe(&rest[..mid], &rest[mid..], sel & 2 != 0, u64::from(sel) << 10);
    let _ = nzbget_rs::deadvolume::rar5_part_crc(rest);
    let _ = nzbget_rs::deadvolume::rar5_head_from_tail(rest, u64::from(sel) * 4096, usize::from(sel & 3));
    let _ = nzbget_rs::deadvolume::rar5_tail_is_rebuilt(rest);
});
