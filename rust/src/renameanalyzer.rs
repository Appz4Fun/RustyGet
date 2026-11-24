//! DirectRenamer's RenameContentAnalyzer (DirectRenamer.cpp): the first
//! bytes of a downloaded file - whether it starts with a PAR2 packet (and its
//! set id) and the MD5 of its first 16 KiB - fed article by article. Recognize
//! the header on the piece that completes it, before QueueCoordinator reads
//! the PAR flag. Reset keeps the identity already found, as in C++.

#[cfg(test)]
use md5::{Digest, Md5};

#[cfg(test)]
const HEADER: usize = 64;
#[cfg(test)]
const MAGIC: &[u8; 8] = b"PAR2\0PKT";

/// Par2::MD5Hash::print: the bytes from last to first, upper-case hex.
pub(crate) fn print(hash: &[u8]) -> [u8; 33] {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = [0; 33];
    for (pair, b) in out[..32]
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip(hash.iter().rev())
    {
        pair[0] = HEX[(b >> 4) as usize];
        pair[1] = HEX[(b & 15) as usize];
    }
    out
}

/// `par_rs::LegacyHeadAnalyzer` (par-rs #82/#90: the exact legacy behaviour, no allocations),
/// printed the way nzbget prints it: digests and set ids byte-reversed, upper-case hex.
#[derive(Default)]
pub struct Analyzer(par_rs::LegacyHeadAnalyzer);

impl Analyzer {
    /// Reset: a new MD5 and size (what was found and the header bytes stay)
    pub fn reset(&mut self) {
        self.0.reset();
    }

    /// Append; true when it (re)recognized a PAR2 packet header
    pub fn append(&mut self, buf: &[u8]) -> bool {
        self.0.append(buf)
    }

    /// Finish: the MD5 of the first 16 KiB, printed
    pub fn finish(&mut self) -> [u8; 33] {
        print(&self.0.finish())
    }

    /// Whether the file starts with a PAR2 packet header.
    pub fn par_file(&self) -> bool {
        self.0.par_file()
    }

    /// The set id of that header, printed.
    pub fn par_set_id(&self) -> Option<[u8; 33]> {
        self.0.par_set_id().map(|id| print(&id.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn par_header_and_hash() {
        let mut header = [0u8; 64];
        header[..8].copy_from_slice(MAGIC);
        for (i, b) in header[32..48].iter_mut().enumerate() {
            *b = i as u8;
        }
        let mut a = Analyzer::default();
        assert!(a.append(&header));
        assert!(a.par_file());
        assert!(a.append(b"more"));
        assert_eq!(
            a.par_set_id().as_ref(),
            Some(b"0F0E0D0C0B0A09080706050403020100\0")
        );
        let mut b = Analyzer::default();
        b.append(b"");
        // MD5("") = d41d8cd98f00b204e9800998ecf8427e, printed backwards
        assert_eq!(&b.finish(), b"7E42F8EC980980E904B2008FD98C1DD4\0");
    }

    // The 32-bit size wrap is covered by par-rs
    // (`hasher::legacy_head::tests::size_wrap_keeps_the_legacy_window`).

    #[test]
    fn reset_preserves_par_recognition_and_empty_inputs() {
        let mut a = Analyzer::default();
        assert!(!a.append(b""));
        assert_eq!(&a.finish(), b"7E42F8EC980980E904B2008FD98C1DD4\0");

        a.reset();
        let mut header = [0u8; 64];
        header[..8].copy_from_slice(MAGIC);
        for (i, b) in header[32..48].iter_mut().enumerate() {
            *b = i as u8;
        }
        assert!(a.append(&header));
        assert!(a.append(b""));
        assert_eq!(
            a.par_set_id().as_ref(),
            Some(b"0F0E0D0C0B0A09080706050403020100\0")
        );
        a.reset();
        assert!(a.par_file());
        assert_eq!(
            a.par_set_id().as_ref(),
            Some(b"0F0E0D0C0B0A09080706050403020100\0")
        );
    }

    #[test]
    fn complete_par_header_is_recognized_in_its_first_piece() {
        for size in [HEADER, 2472] {
            let mut bytes = vec![0u8; size];
            bytes[..8].copy_from_slice(MAGIC);
            for (i, b) in bytes[32..48].iter_mut().enumerate() {
                *b = i as u8;
            }
            let mut a = Analyzer::default();
            assert!(
                a.append(&bytes),
                "complete PAR2 header in one piece must be visible immediately"
            );
            assert!(a.par_file());
            assert_eq!(
                a.par_set_id().as_ref(),
                Some(b"0F0E0D0C0B0A09080706050403020100\0")
            );
            assert_eq!(a.finish(), print(&Md5::digest(&bytes)));
        }
    }

    #[test]
    fn split_header_is_recognized_when_the_last_header_byte_arrives() {
        let mut header = [0u8; HEADER];
        header[..8].copy_from_slice(MAGIC);
        for split in [HEADER - 1, 47] {
            let mut a = Analyzer::default();
            assert!(!a.append(&header[..split]));
            assert!(!a.par_file());
            assert!(a.append(&header[split..]));
            assert!(a.par_file());
            assert_eq!(a.finish(), print(&Md5::digest(header)));
        }
    }

    #[test]
    fn partial_or_nonpar_first_piece_remains_unrecognized() {
        let mut header = [0u8; HEADER];
        header[..8].copy_from_slice(MAGIC);
        for length in 0..HEADER {
            let mut a = Analyzer::default();
            assert!(!a.append(&header[..length]));
            assert!(!a.par_file());
        }
        let mut a = Analyzer::default();
        assert!(!a.append(&[b'x'; 2472]));
        assert!(!a.par_file());
    }

    #[test]
    fn direct_renamer_content_full_suite_port_from_cpp() {
        let mut header = [0u8; HEADER];
        header[..8].copy_from_slice(MAGIC);

        // 1. CompleteHeaderIsVisibleBeforeArticleDownloaded
        for size in [64usize, 2472usize] {
            let mut a = Analyzer::default();
            let mut bytes = vec![0u8; size];
            bytes[..HEADER].copy_from_slice(&header);
            a.reset();
            assert!(a.append(&bytes));
            assert!(a.par_file());
        }

        // 2. SplitHeaderIsVisibleOnItsCompletingPiece
        for split in 1..HEADER {
            let mut a = Analyzer::default();
            a.reset();
            assert!(!a.append(&header[..split]));
            assert!(!a.par_file());
            assert!(a.append(&header[split..]));
            assert!(a.par_file());
        }

        // 3. CommittedParIndexIsVisibleFromOneAppend
        let par_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/testdata/parchecker/testfile.par2");
        let bytes = std::fs::read(&par_path).unwrap();
        assert_eq!(bytes.len(), 4608);
        assert_eq!(&bytes[..8], MAGIC);
        let mut a = Analyzer::default();
        a.reset();
        assert!(a.append(&bytes));
        assert!(a.par_file());

        // 4. PartialAndNonParDataCannotAuthorizeBorrowing
        for size in 0..HEADER {
            let mut a = Analyzer::default();
            a.reset();
            assert!(!a.append(&header[..size]));
            assert!(!a.par_file());
        }
        for byte in 0..8 {
            let mut invalid = header;
            invalid[byte] ^= 1;
            let mut a = Analyzer::default();
            a.reset();
            assert!(!a.append(&invalid));
            assert!(!a.par_file());
        }
    }
}
