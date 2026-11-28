#![no_main]
use libfuzzer_sys::fuzz_target;
use nzbget_rs::nzbfile::{Ctype, NzbFileParser};

fuzz_target!(|data: &[u8]| {
    let mut classes = [0u16; 256];
    for (c, slot) in classes.iter_mut().enumerate() {
        let b = c as u8;
        *slot = u16::from(b.is_ascii_whitespace() || b == 0x0b) | (u16::from(b.is_ascii_digit()) << 1);
    }
    let lower: Vec<i32> = (-128..256i32)
        .map(|c| if (65..=90).contains(&c) { c + 32 } else { c })
        .collect();
    let ct = Ctype { classes: &classes, space: 1, digit: 2, lower: &lower, char_signed: true };
    let mut parser = NzbFileParser::new(b"fuzz.nzb");
    let mut id = 0;
    let ret = parser.parse_xml(data, || {
        id += 1;
        id
    });
    let _ = parser.finalize(ret, b"par2", &ct);
});
