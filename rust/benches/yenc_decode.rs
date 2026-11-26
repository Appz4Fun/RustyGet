//! yEnc article decode throughput through the real `Decoder` caller:
//! framing, dot-unstuffing, the SIMD body kernel and the CRC-32 check.
//! `cargo bench --bench yenc_decode` prints MB/s of encoded input per case.
use nzbget_rs::decoder::{yenc_crc, yenc_decode_piece, Decoder, Status};
use std::time::Instant;

/// A 750,000-byte part encoded as a news server sends it: 128-byte lines,
/// critical characters escaped, leading dots doubled.
fn article(seed: u64, size: usize) -> (Vec<u8>, u32) {
    let mut x = seed;
    let data: Vec<u8> = (0..size)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    let crc = crc32fast::hash(&data);
    let mut out = format!(
        "=ybegin part=1 line=128 size={size} name=bench.bin\r\n=ypart begin=1 end={size}\r\n"
    )
    .into_bytes();
    let mut col = 0;
    for &b in &data {
        let e = b.wrapping_add(42);
        let esc = matches!(e, 0 | b'\n' | b'\r' | b'=')
            || (col == 0 && matches!(e, b'\t' | b' ' | b'.'))
            || (col == 127 && matches!(e, b'\t' | b' '));
        if esc {
            out.push(b'=');
            out.push(e.wrapping_add(64));
            col += 2;
        } else {
            out.push(e);
            col += 1;
        }
        if col >= 128 {
            out.extend_from_slice(b"\r\n");
            col = 0;
        }
    }
    out.extend_from_slice(
        format!("\r\n=yend size={size} part=1 pcrc32={crc:08x}\r\n.\r\n").as_bytes(),
    );
    (out, crc)
}

fn run(article: &[u8], chunk: usize, scratch: &mut Vec<u8>) -> (usize, Status, u32) {
    let mut d = Decoder::new(yenc_decode_piece, yenc_crc);
    d.crc_check = true;
    let mut total = 0;
    for piece in article.chunks(chunk) {
        scratch.clear();
        scratch.extend_from_slice(piece);
        scratch.resize(piece.len() + 128, 0);
        total += d.decode_buffer(scratch, piece.len());
    }
    (total, d.check(), d.calculated_crc)
}

fn main() {
    let quick = std::env::args().any(|a| a == "--quick" || a == "--test");
    let size = 750_000;
    let (art, crc) = article(0x9e3779b97f4a7c15, size);
    let mut scratch = Vec::with_capacity(art.len() + 256);
    let (n, st, got) = run(&art, 16384, &mut scratch);
    assert_eq!((n, st, got), (size, Status::Finished, crc), "decode check");
    if std::env::var_os("YENC_PARTS").is_some() {
        let data: Vec<u8> = art.iter().map(|b| b.wrapping_add(1)).collect();
        let t = Instant::now();
        for _ in 0..200 {
            std::hint::black_box(crc32fast::hash(std::hint::black_box(&data)));
        }
        println!(
            "crc32fast: {:.1} MB/s",
            data.len() as f64 * 200.0 / t.elapsed().as_secs_f64() / 1e6
        );
        // the body alone: from after the =ypart line to the =yend line, as the
        // Decoder hands it to its kernel
        let start = art
            .windows(7)
            .position(|w| w == b"\r\n=ypar")
            .and_then(|p| art[p + 2..].windows(2).position(|w| w == b"\r\n").map(|q| p + 2 + q + 2))
            .expect("=ypart line");
        let end = art
            .windows(7)
            .rposition(|w| w == b"\r\n=yend")
            .expect("=yend line");
        let body = &art[start..end];
        let mut buf = body.to_vec();
        let mut state = 0;
        let step = nzbget_rs::decoder::yenc_decode_piece(&mut buf, 0, body.len(), 0, &mut state);
        assert_eq!(step.dst, size, "the body kernel decodes the whole part");
        let t = Instant::now();
        for _ in 0..200 {
            buf.copy_from_slice(body);
            let mut state = 0;
            std::hint::black_box(nzbget_rs::decoder::yenc_decode_piece(
                &mut buf,
                0,
                body.len(),
                0,
                &mut state,
            ));
        }
        println!(
            "copy+yenc_decode_piece on the body (the Decoder's kernel, no framing, no CRC): {:.1} MB/s",
            body.len() as f64 * 200.0 / t.elapsed().as_secs_f64() / 1e6
        );
        let mut whole = art.clone();
        let t = Instant::now();
        for _ in 0..200 {
            whole.copy_from_slice(std::hint::black_box(&art));
            std::hint::black_box(&whole);
        }
        println!(
            "copy only: {:.1} MB/s",
            art.len() as f64 * 200.0 / t.elapsed().as_secs_f64() / 1e6
        );
    }
    for chunk in [4096usize, 16384, 65536, art.len()] {
        let iters = if quick { 3 } else { 400 };
        let mut best = f64::MAX;
        for _ in 0..5 {
            let t = Instant::now();
            for _ in 0..iters / 5 {
                std::hint::black_box(run(&art, chunk, &mut scratch));
            }
            best = best.min(t.elapsed().as_secs_f64() / (iters / 5) as f64);
        }
        println!(
            "yenc_decode chunk={chunk:>7}: {:8.1} MB/s (SIMD cap setting {})",
            art.len() as f64 / best / 1e6,
            std::env::var("NNTP_RS_SIMD")
                .or_else(|_| std::env::var("RUSTYGET_SIMD"))
                .unwrap_or_else(|_| "auto".into())
        );
    }
}
