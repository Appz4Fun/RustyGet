#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = nzbget_rs::parrenamer::parse_par2_packets(data);
    let _ = nzbget_rs::twin::par2_set(data);
    let _ = nzbget_rs::parchecker::load_par2_set_bytes(data);
    // the same bytes as two pieces of a volume at an offset, as live salvage gets them
    let mid = data.len() / 2;
    let pieces: [(usize, u64, &[u8]); 2] = [(0, 0, &data[..mid]), (0, mid as u64, &data[mid..])];
    let _ = nzbget_rs::fuzz_entry::par2_salvage(&pieces);
});
