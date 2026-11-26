//! The article decoder (Decoder) comes from nntp-rs: it finds yEnc or
//! UU-encoded data in an article as it arrives in pieces, decodes it into the
//! caller's buffer and checks it (sizes, CRC). `yenc_decode_piece` decodes
//! the yEnc body (vector kernels for the middle of lines, chosen at run time;
//! `RUSTYGET_SIMD`, or nntp-rs's own `NNTP_RS_SIMD`, caps them) and
//! `yenc_crc` checks it. The Decoder is RustyGet's own, carried into
//! nntp-rs (its tests moved with it); the body kernel is new, and a
//! dot-stuffed UU line now decodes right (see the test below).

pub use nntp_rs::decoder::{
    yenc_crc, yenc_decode_piece, CrcFn, DecodeFn, Decoder, Format, PieceEnd, PieceStep, Status,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// One yEnc part as a server sends it (dot-stuffed, `.` CR LF at the end),
    /// fed to the Decoder in uneven pieces the way the download path does.
    #[test]
    fn decodes_a_dot_stuffed_part_in_pieces() {
        let data: Vec<u8> = (0..20_000u32).map(|i| (i * 7 + i / 13) as u8).collect();
        let size = data.len();
        let crc = crc32fast::hash(&data);
        let mut art = format!(
            "=ybegin part=1 line=128 size={size} name=t.bin\r\n=ypart begin=1 end={size}\r\n"
        )
        .into_bytes();
        let mut col = 0;
        for &b in &data {
            let e = b.wrapping_add(42);
            if matches!(e, 0 | b'\n' | b'\r' | b'=') {
                art.extend_from_slice(&[b'=', e.wrapping_add(64)]);
                col += 2;
            } else {
                if col == 0 && e == b'.' {
                    art.push(b'.');
                }
                art.push(e);
                col += 1;
            }
            if col >= 128 {
                art.extend_from_slice(b"\r\n");
                col = 0;
            }
        }
        art.extend_from_slice(
            format!("\r\n=yend size={size} part=1 pcrc32={crc:08x}\r\n.\r\n").as_bytes(),
        );

        for piece in [1usize, 3, 61, 4096, art.len()] {
            let mut d = Decoder::new(yenc_decode_piece, yenc_crc);
            d.crc_check = true;
            let mut out = Vec::new();
            let mut scratch = Vec::new();
            for chunk in art.chunks(piece) {
                scratch.clear();
                scratch.extend_from_slice(chunk);
                scratch.resize(chunk.len() + 128, 0);
                let n = d.decode_buffer(&mut scratch, chunk.len());
                out.extend_from_slice(&scratch[..n]);
            }
            assert_eq!(d.check(), Status::Finished, "piece {piece}");
            assert_eq!(d.format, Format::Yenc);
            assert_eq!(d.calculated_crc, crc, "piece {piece}");
            assert_eq!(out, data, "piece {piece}");
        }
    }

    /// A UU-encoded article whose lines include a 14-byte one: its length
    /// character is `.`, so it travels dot-stuffed as `..` and must decode
    /// to the same bytes as any other line (fixed in nntp-rs v0.3.2).
    #[test]
    fn decodes_a_dot_stuffed_uu_line() {
        let enc = |v: u8| if v == 0 { b'`' } else { v + 32 };
        let uu_line = |data: &[u8]| {
            let mut line = vec![enc(data.len() as u8)];
            for g in data.chunks(3) {
                let b = [g[0], *g.get(1).unwrap_or(&0), *g.get(2).unwrap_or(&0)];
                line.extend([
                    enc(b[0] >> 2),
                    enc(((b[0] & 3) << 4) | (b[1] >> 4)),
                    enc(((b[1] & 15) << 2) | (b[2] >> 6)),
                    enc(b[2] & 63),
                ]);
            }
            line
        };
        let data: Vec<u8> = (0..45u8 + 45 + 14).map(|i| i.wrapping_mul(37)).collect();
        let mut wire = b"begin 644 uu.bin\r\n".to_vec();
        for chunk in [&data[..45], &data[45..90], &data[90..]] {
            let line = uu_line(chunk);
            if line[0] == b'.' {
                wire.push(b'.');
            }
            wire.extend_from_slice(&line);
            wire.extend_from_slice(b"\r\n");
        }
        wire.extend_from_slice(b"`\r\nend\r\n.\r\n");
        assert!(wire.windows(3).any(|w| w == b"\n.."), "the 14-byte line is dot-stuffed");

        for piece in [1usize, 7, wire.len()] {
            let mut d = Decoder::new(yenc_decode_piece, yenc_crc);
            let mut out = Vec::new();
            let mut scratch = Vec::new();
            for chunk in wire.chunks(piece) {
                scratch.clear();
                scratch.extend_from_slice(chunk);
                scratch.resize(chunk.len() + 128, 0);
                let n = d.decode_buffer(&mut scratch, chunk.len());
                out.extend_from_slice(&scratch[..n]);
            }
            assert_eq!(d.format, Format::Ux, "piece {piece}");
            assert_eq!(d.check(), Status::Finished, "piece {piece}");
            assert_eq!(out, data, "piece {piece}");
        }
    }

    /// What `simd_cap_is_honoured` checks in a child process: the decoder's
    /// payload kernel and break scan under the cap in `RUSTYGET_SIMD`, and a
    /// part still decoding to the right bytes there.
    #[test]
    #[ignore = "run by simd_cap_is_honoured in a child process"]
    fn simd_cap_child() {
        let cap = std::env::var("RUSTYGET_SIMD").unwrap_or_default();
        let payload = nntp_rs::yenc_simd::kernel();
        let scan = nntp_rs::decoder::scan_kernel();
        match cap.as_str() {
            // 0: the scalar payload kernel; 1: the scalar break scan (nntp-rs
            // v0.4.0 caps the break scan on x86 only; NEON's ignores the cap)
            "scalar" => {
                assert_eq!(payload, 0, "cap {cap}");
                if cfg!(target_arch = "x86_64") {
                    assert_eq!(scan, 1, "cap {cap}");
                }
            }
            // x86: at most AVX2 (payload 2, scan 2)
            "avx2" if cfg!(target_arch = "x86_64") => {
                assert!(payload <= 2 && scan <= 2, "cap {cap}: payload {payload}, scan {scan}")
            }
            _ => {}
        }
        decodes_a_dot_stuffed_part_in_pieces();
    }

    /// `RUSTYGET_SIMD` caps nntp-rs's kernels (read once per process, so
    /// each cap runs in a process of its own).
    #[test]
    fn simd_cap_is_honoured() {
        let exe = std::env::current_exe().expect("test binary");
        for cap in ["scalar", "avx2"] {
            let out = std::process::Command::new(&exe)
                .args(["--exact", "decoder::tests::simd_cap_child", "--ignored", "--test-threads=1"])
                .env("RUSTYGET_SIMD", cap)
                .env_remove("NNTP_RS_SIMD")
                .output()
                .expect("run the child test");
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success() && text.contains("1 passed"),
                "cap {cap}: {text}{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}
