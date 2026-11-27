#![no_main]
use libfuzzer_sys::fuzz_target;
use nzbget_rs::decoder::{yenc_crc, yenc_decode_piece, Decoder};

// The first byte picks the piece size the way a socket delivers an article in
// uneven reads; the rest is the article as the server sent it.
fuzz_target!(|data: &[u8]| {
    let Some((&sel, art)) = data.split_first() else { return };
    let piece = match sel % 5 {
        0 => 1,
        1 => 7,
        2 => 61,
        3 => 4096,
        _ => art.len().max(1),
    };
    let mut d = Decoder::new(yenc_decode_piece, yenc_crc);
    d.crc_check = sel & 0x80 != 0 || sel & 1 == 0;
    let mut scratch = Vec::new();
    let mut total = 0usize;
    for chunk in art.chunks(piece) {
        scratch.clear();
        scratch.extend_from_slice(chunk);
        // the download path reserves 128 slack bytes after every read
        scratch.resize(chunk.len() + 128, 0);
        total += d.decode_buffer(&mut scratch, chunk.len());
        if d.eof {
            break;
        }
    }
    let _ = total;
});
