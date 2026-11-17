//! Checks ported from the retired C++ harnesses under `rust/tests`.
//!
//! * `latin1_caller/driver.cpp` asserted that the Rust `JsonEncode` and
//!   `Latin1ToUtf8` produced the same bytes as the pre-port C++ bodies on its
//!   benchmark fixtures. The outputs it accepted are kept here as vectors.
//! * `streampatchparts_harness.cpp` compared `nzbget_rs_stream_patch_parts`
//!   with the old C++ `DupeStreamRepair::SelectPatchParts` on random inputs.
//!   The old routine marked every donor that a hole strictly overlaps, plus
//!   `margin` neighbours on each side, in ascending order without duplicates.
//!   That brute-force reference is reproduced below and compared on random
//!   inputs, including the large sorted inputs that take the indexed paths.

use nzbget_rs::escape::json_encode;
use nzbget_rs::streampatchparts::{select, Range};
use nzbget_rs::text::latin1_to_utf8;

fn json(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    json_encode(s, &mut out);
    out
}

#[test]
fn json_encode_matches_old_cpp_fixtures() {
    assert_eq!(
        json(b"Some.Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-GROUP \"quoted\" / x"),
        &b"Some.Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-GROUP \\\"quoted\\\" \\/ x"[..]
    );
    assert_eq!(
        json(b"Caf\xc3\xa9 \xc3\x9cber \xe2\x80\x93 Fran\xc3\xa7ais d\xc3\xa9j\xc3\xa0 vu \xe6\x97\xa5\xe6\x9c\xac"),
        &b"Caf\\u00e9 \\u00dcber \\u2013 Fran\\u00e7ais d\\u00e9j\\u00e0 vu \\u65e5\\u672c"[..]
    );
    let line = b"[INFO] Unpacking \"Show.S01E01.part01.rar\" (12/34)\r\n\tpath/to/file\\x: done\n";
    let encoded: &[u8] = b"[INFO] Unpacking \\\"Show.S01E01.part01.rar\\\" (12\\/34)\\r\\n\\tpath\\/to\\/file\\\\x: done\\n";
    assert_eq!(json(line), encoded);
    // The 4 KiB and 64 KiB fixtures repeat the line; encoding is per byte.
    for size in [4096, 65536] {
        let mut input = Vec::new();
        let mut expected = Vec::new();
        while input.len() < size {
            input.extend_from_slice(line);
            expected.extend_from_slice(encoded);
        }
        assert_eq!(json(&input), expected);
    }
}

#[test]
fn latin1_to_utf8_matches_old_cpp_fixture() {
    let mut out = Vec::new();
    latin1_to_utf8(
        b"Caf\xe9 \xfcber \xe7a va \xe0 la carte - Fran\xe7ais.nzb",
        &mut out,
    );
    assert_eq!(
        out,
        "Caf\u{e9} \u{fc}ber \u{e7}a va \u{e0} la carte - Fran\u{e7}ais.nzb".as_bytes()
    );
}

fn reference(donors: &[Range], holes: &[Range], margin: i32) -> Vec<i32> {
    let mut out = Vec::new();
    if margin < 0 {
        return out;
    }
    let margin = margin as usize;
    let mut next = 0usize;
    for (i, d) in donors.iter().enumerate() {
        let hit = holes
            .iter()
            .any(|h| d.offset < h.offset + h.size && h.offset < d.offset + d.size);
        if hit {
            let from = i.saturating_sub(margin).max(next);
            let to = (i + margin).min(donors.len() - 1);
            out.extend((from..=to).map(|j| j as i32));
            next = to + 1;
        }
    }
    out
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: u64) -> i64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n) as i64
    }
}

#[test]
fn select_patch_parts_matches_old_cpp_reference() {
    let mut rng = Lcg(731921);
    for round in 0..4000 {
        let sorted = round % 2 == 0;
        let donor_count = if round % 5 == 0 {
            40 + rng.next(400)
        } else {
            rng.next(40)
        } as usize;
        let hole_count = rng.next(if round % 3 == 0 { 30 } else { 6 }) as usize;
        let mut donors = Vec::with_capacity(donor_count);
        let mut offset = 0;
        for _ in 0..donor_count {
            let size = rng.next(20) - 2;
            if sorted {
                donors.push(Range {
                    offset,
                    size: size.max(0),
                });
                offset += size.max(0) + rng.next(3);
            } else {
                donors.push(Range {
                    offset: rng.next(2000) - 50,
                    size,
                });
            }
        }
        let span = if sorted {
            offset.max(1) as u64 + 20
        } else {
            2100
        };
        let holes: Vec<Range> = (0..hole_count)
            .map(|_| Range {
                offset: rng.next(span) - 10,
                size: rng.next(30) - 3,
            })
            .collect();
        let margin = rng.next(5) as i32 - 1;
        let mut output = vec![-1; donors.len()];
        let len = select(&donors, &holes, margin, &mut output);
        output.truncate(len);
        assert_eq!(output, reference(&donors, &holes, margin), "round {round}");
    }
}
