//! Which copy of a release goes first in a fleet, and which duplicate is asked
//! first for missing data: TWINs (byte-identical postings, their articles swap
//! one for one), then ALTs whose packaging stream repair can cross-map
//! ("borrowable": bare media, store-mode rar/zip, raw splits), then ALTs whose
//! packaging it cannot read without decompressing or a password
//! ("unborrowable"). A copy whose packaging could not be told sits between the
//! two kinds of ALT.
//!
//! The kind a fleet worked out for a member is kept on the queued item in the
//! nzb parameter [`KIND_PARAM`], so the orderings that pick a duplicate later
//! (article borrowing, stream repair, the rescue of a failed download) agree
//! with the fleet's.

/// The nzb parameter holding a fleet member's [`DonorKind`].
pub const KIND_PARAM: &[u8] = b"DupeKind";

/// How the posted bytes of a copy relate to its inner media file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packing {
    /// The inner file's bytes are posted as they are (bare media, store-mode
    /// rar or zip, a raw split, an encrypted store-mode archive whose password
    /// the nzb carries).
    Borrowable,
    /// Not known: 7-Zip (its headers are at the end), an unrecognised start,
    /// or the first article could not be read.
    Unknown,
    /// Compressed, or encrypted without a known password.
    Unborrowable,
}

/// Priority of a copy, lower first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DonorKind {
    Twin = 0,
    Borrowable = 1,
    Unknown = 2,
    Unborrowable = 3,
}

impl DonorKind {
    /// TWIN outranks packaging: a twin's articles fill holes directly, whatever
    /// its packaging.
    pub fn of(has_twin: bool, packing: Packing) -> Self {
        if has_twin {
            return DonorKind::Twin;
        }
        match packing {
            Packing::Borrowable => DonorKind::Borrowable,
            Packing::Unknown => DonorKind::Unknown,
            Packing::Unborrowable => DonorKind::Unborrowable,
        }
    }

    /// The value stored in [`KIND_PARAM`].
    pub fn param(self) -> &'static [u8] {
        match self {
            DonorKind::Twin => b"twin",
            DonorKind::Borrowable => b"borrowable",
            DonorKind::Unknown => b"unknown",
            DonorKind::Unborrowable => b"unborrowable",
        }
    }

    /// The kind stored in [`KIND_PARAM`]; an item without one (not added by a
    /// fleet) counts as [`DonorKind::Unknown`].
    pub fn from_param(value: Option<&[u8]>) -> Self {
        match value.map(|v| v.trim_ascii()) {
            Some(b"twin") => DonorKind::Twin,
            Some(b"borrowable") => DonorKind::Borrowable,
            Some(b"unborrowable") => DonorKind::Unborrowable,
            _ => DonorKind::Unknown,
        }
    }
}

const MEDIA_EXTS: [&[u8]; 8] = [
    b"mkv", b"mp4", b"m4v", b"avi", b"ts", b"m2ts", b"wmv", b"mov",
];

fn extension(name: &[u8]) -> Vec<u8> {
    match name.iter().rposition(|&b| b == b'.') {
        Some(dot) => name[dot + 1..].to_ascii_lowercase(),
        None => Vec::new(),
    }
}

/// The name of a raw split's piece without its numeric extension
/// (`movie.mkv.001` → `movie.mkv`).
fn split_base(name: &[u8]) -> Option<&[u8]> {
    let dot = name.iter().rposition(|&b| b == b'.')?;
    let ext = &name[dot + 1..];
    (ext.len() == 3 && ext.iter().all(u8::is_ascii_digit)).then_some(&name[..dot])
}

/// Media containers recognisable from their first bytes.
fn media_magic(data: &[u8]) -> bool {
    data.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) // Matroska / WebM (EBML)
        || data.get(4..8) == Some(b"ftyp") // MP4 / MOV
        || (data.starts_with(b"RIFF") && data.get(8..12) == Some(b"AVI "))
        || (data.len() > 188 && data[0] == 0x47 && data[188] == 0x47) // MPEG-TS
        || (data.len() > 196 && data[4] == 0x47 && data[196] == 0x47) // M2TS
        || data.starts_with(&[0x30, 0x26, 0xb2, 0x75]) // ASF / WMV
}

struct MemSource<'a>(&'a [u8]);

impl crate::rarreader::Source for MemSource<'_> {
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
        match self.0.get(start..end) {
            Some(slice) => {
                buf.copy_from_slice(slice);
                true
            }
            None => false,
        }
    }
}

/// Headers are only read without a password here: an encrypted header stops
/// the reader before it asks for a key.
struct NoCipher;

impl crate::rarreader::Cipher for NoCipher {
    fn rar3_prepare(&mut self, _salt: &[u8; 8]) -> bool {
        false
    }
    fn rar5_prepare(&mut self, _kdf_count: u8, _salt: &[u8; 16]) -> bool {
        false
    }
    fn init(&mut self, _key_bits: i32, _iv: Option<&[u8; 16]>) -> bool {
        false
    }
    fn decrypt(&mut self, _input: &[u8; 16], _output: &mut [u8; 16]) -> bool {
        false
    }
    fn free(&mut self) {}
}

fn rar_packing(data: &[u8], password_known: bool) -> Packing {
    let mut volume = crate::rarreader::Volume::default();
    let mut files: Vec<crate::rarreader::RarFile> = Vec::new();
    // The decoded article ends inside the first file's data, so the reader
    // stops there; the file header it read before that is what counts.
    crate::rarreader::read_from(&mut volume, &mut MemSource(data), &mut NoCipher, &mut files);
    if volume.encrypted {
        // encrypted headers: nothing about the content can be read without the
        // password, and the repair needs it to map the archive
        return if password_known {
            Packing::Unknown
        } else {
            Packing::Unborrowable
        };
    }
    let Some(file) = files.first() else {
        return Packing::Unknown;
    };
    if !file.stored || (file.encrypted_data && !password_known) {
        Packing::Unborrowable
    } else {
        Packing::Borrowable
    }
}

fn zip_packing(data: &[u8], password_known: bool) -> Packing {
    let (Some(flags), Some(method)) = (data.get(6..8), data.get(8..10)) else {
        return Packing::Unknown;
    };
    let flags = u16::from_le_bytes([flags[0], flags[1]]);
    let method = u16::from_le_bytes([method[0], method[1]]);
    if method != 0 || (flags & 1 != 0 && !password_known) {
        Packing::Unborrowable
    } else {
        Packing::Borrowable
    }
}

/// The packaging of a copy, read from the decoded first article of its
/// largest data file (`name` is that file's name in the nzb).
/// `password_known`: the nzb carries the archive's password.
pub fn packing_of_head(data: &[u8], name: &[u8], password_known: bool) -> Packing {
    if data.starts_with(b"Rar!\x1a\x07") {
        return rar_packing(data, password_known);
    }
    if data.starts_with(b"PK\x03\x04") {
        return zip_packing(data, password_known);
    }
    if data.starts_with(&[b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c]) {
        return Packing::Unknown;
    }
    let ext = extension(name);
    if MEDIA_EXTS.contains(&ext.as_slice()) || media_magic(data) {
        return Packing::Borrowable;
    }
    if let Some(base) = split_base(name) {
        if MEDIA_EXTS.contains(&extension(base).as_slice()) {
            return Packing::Borrowable;
        }
    }
    Packing::Unknown
}

/// Whether two copies are TWINs: their par2 sets list the same files (by MD5
/// and length; a near-twin sharing most files counts too, as for article
/// borrowing), or, when either has no complete par2 list, they hold the same
/// number of data files with the same total size.
pub fn twins(
    a: &crate::twin::Listed,
    b: &crate::twin::Listed,
    a_data: (i64, i32),
    b_data: (i64, i32),
) -> bool {
    if a.complete && b.complete {
        return crate::twin::kind(a, b).starts_with("twin")
            || crate::twin::kind(b, a).starts_with("twin");
    }
    let (a_bytes, a_files) = a_data;
    let (b_bytes, b_files) = b_data;
    a_bytes > 0
        && a_files > 0
        && crate::dupeidentity::same_data(
            a_bytes as u64,
            a_files as u32,
            b_bytes as u64,
            b_files as u32,
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twin::{FileSig, Listed};

    fn rar4_with_file(method: u8, file_flags: u16, main_flags: u16) -> Vec<u8> {
        let mut v = b"Rar!\x1a\x07\x00".to_vec();
        // main header: crc, type 0x73, flags, size 13
        v.extend_from_slice(&[0, 0, 0x73]);
        v.extend_from_slice(&main_flags.to_le_bytes());
        v.extend_from_slice(&13u16.to_le_bytes());
        v.extend_from_slice(&[0; 6]);
        let name = b"movie.mkv";
        let size = 32 + name.len() as u16;
        v.extend_from_slice(&[0, 0, 0x74]);
        v.extend_from_slice(&(file_flags | 0x8000).to_le_bytes());
        v.extend_from_slice(&size.to_le_bytes());
        v.extend_from_slice(&1_000_000u32.to_le_bytes()); // packed
        v.extend_from_slice(&1_000_000u32.to_le_bytes()); // unpacked
        v.push(3); // host os
        v.extend_from_slice(&[0; 4]); // crc
        v.extend_from_slice(&[0; 4]); // time
        v.push(29); // version
        v.push(method);
        v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        v.extend_from_slice(&[0; 4]); // attr
        v.extend_from_slice(name);
        v.extend_from_slice(&[0xaa; 64]); // start of the packed data
        v
    }

    #[test]
    fn rar_store_compressed_and_encrypted() {
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0, 0), b"a.rar", false),
            Packing::Borrowable
        );
        assert_eq!(
            packing_of_head(&rar4_with_file(0x33, 0, 0), b"a.rar", false),
            Packing::Unborrowable
        );
        // encrypted file data, store mode: borrowable only with the password
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0x04, 0), b"a.rar", false),
            Packing::Unborrowable
        );
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0x04, 0), b"a.rar", true),
            Packing::Borrowable
        );
        // encrypted headers
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0, 0x80), b"a.rar", false),
            Packing::Unborrowable
        );
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0, 0x80), b"a.rar", true),
            Packing::Unknown
        );
        // a header cut short tells nothing
        assert_eq!(
            packing_of_head(b"Rar!\x1a\x07\x00\x00", b"a.rar", false),
            Packing::Unknown
        );
    }

    #[test]
    fn zip_7z_and_bare() {
        let mut zip = b"PK\x03\x04\x14\x00\x00\x00\x00\x00".to_vec();
        zip.extend_from_slice(&[0; 20]);
        assert_eq!(packing_of_head(&zip, b"a.zip", false), Packing::Borrowable);
        zip[8] = 8; // deflate
        assert_eq!(
            packing_of_head(&zip, b"a.zip", false),
            Packing::Unborrowable
        );
        zip[8] = 0;
        zip[6] = 1; // encrypted
        assert_eq!(
            packing_of_head(&zip, b"a.zip", false),
            Packing::Unborrowable
        );
        assert_eq!(packing_of_head(&zip, b"a.zip", true), Packing::Borrowable);
        assert_eq!(
            packing_of_head(&[b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c, 0, 4], b"a.7z", false),
            Packing::Unknown
        );
        assert_eq!(
            packing_of_head(b"anything", b"Movie.2020.mkv", false),
            Packing::Borrowable
        );
        assert_eq!(
            packing_of_head(&[0x1a, 0x45, 0xdf, 0xa3, 1, 2], b"abc123", false),
            Packing::Borrowable
        );
        assert_eq!(
            packing_of_head(b"anything", b"Movie.2020.mkv.001", false),
            Packing::Borrowable
        );
        assert_eq!(
            packing_of_head(b"anything", b"setup.exe", false),
            Packing::Unknown
        );
    }

    #[test]
    fn kind_order_and_param() {
        assert!(DonorKind::Twin < DonorKind::Borrowable);
        assert!(DonorKind::Borrowable < DonorKind::Unknown);
        assert!(DonorKind::Unknown < DonorKind::Unborrowable);
        assert_eq!(DonorKind::of(true, Packing::Unborrowable), DonorKind::Twin);
        assert_eq!(
            DonorKind::of(false, Packing::Borrowable),
            DonorKind::Borrowable
        );
        for k in [
            DonorKind::Twin,
            DonorKind::Borrowable,
            DonorKind::Unknown,
            DonorKind::Unborrowable,
        ] {
            assert_eq!(DonorKind::from_param(Some(k.param())), k);
        }
        assert_eq!(DonorKind::from_param(None), DonorKind::Unknown);
        assert_eq!(DonorKind::from_param(Some(b"junk")), DonorKind::Unknown);
    }

    fn listed(files: &[(&str, u64, &str)], complete: bool) -> Listed {
        Listed {
            sigs: files
                .iter()
                .map(|&(name, length, md5)| FileSig {
                    name: name.into(),
                    length,
                    md5: md5.into(),
                })
                .collect(),
            complete,
            print: String::new(),
        }
    }

    #[test]
    fn twins_by_par2_or_size() {
        let a = listed(&[("a.r00", 10, "m1"), ("a.r01", 10, "m2")], true);
        let b = listed(&[("b.r00", 10, "m1"), ("b.r01", 10, "m2")], true);
        let c = listed(&[("c.r00", 10, "x1"), ("c.r01", 10, "x2")], true);
        assert!(twins(&a, &b, (20, 2), (20, 2)));
        // complete lists that differ win over equal sizes
        assert!(!twins(&a, &c, (20, 2), (20, 2)));
        // without a complete list, equal data geometry decides
        let none = listed(&[], false);
        assert!(twins(&none, &c, (20, 2), (20, 2)));
        assert!(!twins(&none, &c, (20, 2), (21, 2)));
        assert!(!twins(&none, &none, (0, 0), (0, 0)));
    }
}
