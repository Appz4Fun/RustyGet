//! Entry points for the cargo-fuzz targets in `rust/fuzz`: RustyGet's own
//! header readers that are `pub(crate)`. Compiled only with the `fuzzing`
//! feature.

use crate::rarreader::{read_from, Source, Volume};

struct Bytes<'a>(&'a [u8]);

impl Source for Bytes<'_> {
    fn size(&mut self) -> i64 {
        self.0.len() as i64
    }
    fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool {
        let Ok(start) = usize::try_from(offset) else {
            return false;
        };
        let Some(end) = start.checked_add(buf.len()) else {
            return false;
        };
        let Some(bytes) = self.0.get(start..end) else {
            return false;
        };
        buf.copy_from_slice(bytes);
        true
    }
}

/// `RarVolume::ReadFrom` over in-memory bytes; the number of files read.
pub fn rar_read(data: &[u8], password: Option<&[u8]>) -> (bool, usize) {
    let mut volume = Volume {
        has_password: password.is_some(),
        ..Volume::default()
    };
    let mut files = Vec::new();
    let ok = read_from(&mut volume, &mut Bytes(data), password, &mut files);
    (ok, files.len())
}

/// TwinCheck's own readers of RAR header bytes.
pub fn rar_twin_probe(first: &[u8], last: &[u8], single: bool, min_size: u64) {
    let _ = crate::twinfields::rerar_member_end(first, last, single);
    let _ = crate::twinfields::creation_of(first);
    let _ = crate::twinfields::rar_last_volume_key_with(
        first,
        last.len() as u64,
        single,
        min_size,
        None,
    );
    let _ = crate::twinfields::rar_last_volume_key_with(
        first,
        u64::MAX / 2,
        !single,
        min_size,
        Some("pw"),
    );
}

/// Live salvage of par2 packets from pieces of a volume.
pub fn par2_salvage(pieces: &[(usize, u64, &[u8])]) -> bool {
    crate::twinfields::salvage_par2(pieces).is_some()
}
