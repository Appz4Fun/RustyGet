//! Which copy of a release goes first in a fleet, and which duplicate is asked
//! first for missing data: TWINs (byte-identical postings, their articles swap
//! one for one), then ALTs whose packaging stream repair can cross-map
//! ("borrowable": bare media, store-mode rar/zip, raw splits), then encrypted
//! store-mode RARs whose password the nzb carries ("borrowable-encrypted":
//! stream repair decrypts them on the way, at a cost, so they come after the
//! plain ones), then ALTs whose packaging it cannot read without decompressing
//! or a password ("unborrowable"). A copy whose packaging could not be told
//! sits between the borrowable and the unborrowable ALTs.
//!
//! The kind a fleet worked out for a member is kept on the queued item in the
//! nzb parameter [`KIND_PARAM`] (not `DupeKind`, which TwinCheck owns for its
//! twin/alt label against the primary), so the orderings that pick a duplicate later
//! (article borrowing, stream repair, the rescue of a failed download) agree
//! with the fleet's.

/// The nzb parameter holding a fleet member's [`DonorKind`].
pub const KIND_PARAM: &[u8] = b"DupeCopyKind";

/// The nzb parameter a fleet sets (`yes`) on a copy whose archive headers are
/// encrypted. Whether that locks the copy is decided when it is ranked (see
/// [`locked`]): only while no password is at hand.
pub const LOCKED_PARAM: &[u8] = b"DupeLocked";

/// The nzb parameter holding the archive password (from the nzb's meta, a
/// `{{password}}` in its name, or set later by the user).
pub const PASSWORD_PARAM: &[u8] = b"*Unpack:Password";

/// Whether a [`LOCKED_PARAM`] value says the headers are encrypted.
pub fn encrypted_param(value: Option<&[u8]>) -> bool {
    value.is_some_and(|v| v.trim_ascii().eq_ignore_ascii_case(b"yes"))
}

/// Whether a copy is locked: its archive headers are encrypted and no password
/// is at hand, neither its own (`password_known`) nor one `UnpackPassFile` may
/// give. A locked copy can neither be extracted nor borrowed from, so it ranks
/// below every copy that can, whatever its health, and is taken only when
/// nothing else is alive (the user's rule, 2026-11-19).
pub fn locked(headers_encrypted: bool, password_known: bool) -> bool {
    locked_with(
        headers_encrypted,
        password_known,
        crate::options::unpack_pass_file_set(),
    )
}

/// [`locked`] with the `UnpackPassFile` state given.
pub fn locked_with(headers_encrypted: bool, password_known: bool, pass_file_set: bool) -> bool {
    headers_encrypted && !password_known && !pass_file_set
}

/// [`locked`] for a queued item from its [`LOCKED_PARAM`] and
/// [`PASSWORD_PARAM`] values.
pub fn locked_params(locked_value: Option<&[u8]>, password_value: Option<&[u8]>) -> bool {
    locked(
        encrypted_param(locked_value),
        password_value.is_some_and(|p| !p.trim_ascii().is_empty()),
    )
}

/// How the posted bytes of a copy relate to its inner media file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packing {
    /// The inner file's bytes are posted as they are (bare media, store-mode
    /// rar or zip, a raw split).
    Borrowable,
    /// A store-mode RAR whose file data, and maybe headers
    /// (`headers_encrypted`), are encrypted, and whose password the nzb
    /// carries and opens its headers: stream repair decrypts its bytes on the
    /// way, so it lends like a [`Packing::Borrowable`] copy, ranked after them.
    EncryptedBorrowable { headers_encrypted: bool },
    /// Not known: 7-Zip without a password (its headers are at the end), an
    /// unrecognised start, or the first article could not be read (except a
    /// 7-Zip whose nzb carries a password, which is unborrowable either way).
    Unknown,
    /// Compressed, encrypted without a known password, or a 7-Zip whose nzb
    /// carries a password (encrypted: borrowing means decrypting it whole).
    Unborrowable,
    /// RAR headers encrypted and not readable: no password, a password that
    /// does not open them, or the media inside is compressed or could not be
    /// found. Its kind is unborrowable; without a password it is also
    /// [`locked`]: never extractable, ranked after every copy that can be
    /// extracted (see [`LOCKED_PARAM`]).
    HeadersEncrypted,
}

impl Packing {
    /// Whether the copy's archive headers are encrypted (the fact a fleet
    /// keeps in [`LOCKED_PARAM`]).
    pub fn headers_encrypted(self) -> bool {
        matches!(
            self,
            Packing::HeadersEncrypted
                | Packing::EncryptedBorrowable {
                    headers_encrypted: true
                }
        )
    }
}

/// Priority of a copy, lower first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DonorKind {
    Twin = 0,
    Borrowable = 1,
    /// Borrowable once decrypted with the password its nzb carries (see
    /// [`Packing::EncryptedBorrowable`]): it lends like a borrowable ALT and
    /// ranks right after them, before every copy of unknown packing.
    EncryptedBorrowable = 2,
    Unknown = 3,
    Unborrowable = 4,
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
            Packing::EncryptedBorrowable { .. } => DonorKind::EncryptedBorrowable,
            Packing::Unknown => DonorKind::Unknown,
            Packing::Unborrowable | Packing::HeadersEncrypted => DonorKind::Unborrowable,
        }
    }

    /// The kind for its `as u8` value; anything else is [`DonorKind::Unknown`].
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => DonorKind::Twin,
            1 => DonorKind::Borrowable,
            2 => DonorKind::EncryptedBorrowable,
            4 => DonorKind::Unborrowable,
            _ => DonorKind::Unknown,
        }
    }

    /// The value stored in [`KIND_PARAM`].
    pub fn param(self) -> &'static [u8] {
        match self {
            DonorKind::Twin => b"twin",
            DonorKind::Borrowable => b"borrowable",
            DonorKind::EncryptedBorrowable => b"borrowable-encrypted",
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
            Some(b"borrowable-encrypted") => DonorKind::EncryptedBorrowable,
            Some(b"unborrowable") => DonorKind::Unborrowable,
            _ => DonorKind::Unknown,
        }
    }
}

/// Whether a backup of [`DonorKind::Unknown`] carries a label that makes a borrow from it
/// worth waiting for while the download's extraction runs: TwinCheck labelled it `alt` (another
/// container, the same video inside) or `container` (another container, the video inside NOT
/// verified yet), or a fleet stored an explicit `unknown` kind and found the same video inside
/// (`video`, `DupeVideo=same`). `container` is no proof of the same video: a different encode
/// posted nested, or compressed, under the same dupe key gets it too. It is accepted because
/// failing over throws the running extraction away; the live dead-volume borrow checks the
/// video inside, and once it finds it differs, or finds it compressed (live repair never
/// decompresses, so such a backup cannot lend live), the download fails over, unless another
/// donor lends the dead volumes (`postproc::dead_volume_donor_cannot_lend`). A backup the
/// check can tell nothing of (its reads keep failing, say) holds the download until it ends.
/// An unlabelled backup (a same-key grab of another release or encode) never qualifies, nor
/// one TwinCheck labelled another video (`other`).
pub fn unknown_kind_is_labelled_alt(
    copy_kind: Option<&[u8]>,
    dupe_kind: Option<&[u8]>,
    video: Option<&[u8]>,
) -> bool {
    match dupe_kind.map(|v| v.trim_ascii()) {
        Some(b"alt") | Some(b"container") => true,
        Some(b"other") => false,
        _ => {
            copy_kind.map(|v| v.trim_ascii()) == Some(b"unknown")
                && video.map(|v| v.trim_ascii()) == Some(b"same")
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

/// A media file's name (not a sample).
fn is_media_name(name: &[u8]) -> bool {
    crate::dupeidentity::is_media(name)
}

/// The name of a raw split's piece without its numeric extension
/// (`movie.mkv.001` → `movie.mkv`).
fn split_base(name: &[u8]) -> Option<&[u8]> {
    let dot = name.iter().rposition(|&b| b == b'.')?;
    let ext = &name[dot + 1..];
    (ext.len() == 3 && ext.iter().all(u8::is_ascii_digit)).then_some(&name[..dot])
}

/// Media containers recognisable from their first bytes.
/// The bytes start like a video file. Fleet intake uses it too.
pub(crate) fn media_magic(data: &[u8]) -> bool {
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

fn rar_packing(data: &[u8], password: Option<&[u8]>) -> Packing {
    let password = password.filter(|p| !p.trim_ascii().is_empty());
    let mut volume = crate::rarreader::Volume {
        has_password: password.is_some(),
        ..Default::default()
    };
    let mut files: Vec<crate::rarreader::RarFile> = Vec::new();
    // The decoded article ends inside the first file's data, so the reader
    // stops there; the file header it read before that is what counts.
    // Encrypted headers are read with the nzb's password (zipfast decrypts
    // them); without one, or with one that does not open them, the reader
    // stops at them.
    crate::rarreader::read_from(&mut volume, &mut MemSource(data), password, &mut files);
    let headers_encrypted = volume.encrypted;
    if headers_encrypted && files.is_empty() {
        // encrypted headers nothing opened: the content cannot be told, so
        // it is never taken before a copy that is proven borrowable or
        // simply plain; whether a missing password locks it is told when it
        // is ranked
        return Packing::HeadersEncrypted;
    }
    // the media file decides; a small nfo or sample in front of it says
    // nothing about how the movie is packed
    let Some(file) = files.iter().find(|f| is_media_name(&f.filename)) else {
        return if headers_encrypted {
            Packing::HeadersEncrypted
        } else {
            Packing::Unknown
        };
    };
    if !file.stored {
        // compressed: live repair never decompresses
        if headers_encrypted {
            Packing::HeadersEncrypted
        } else {
            Packing::Unborrowable
        }
    } else if headers_encrypted || file.encrypted_data {
        let Some(pw) = password else {
            return Packing::Unborrowable;
        };
        // Headers that opened prove the password. With only the data
        // encrypted, a RAR5 file header keeps a check value to test it
        // against (zipfast); a RAR4 one keeps none, and stream repair's
        // identity check refuses a wrong password when it borrows.
        if !headers_encrypted && !rar5_password_fits(file, pw) {
            return Packing::Unborrowable;
        }
        // stored but encrypted, password known: the repair decrypts it on
        // the way, at a cost, so it comes after the plain borrowable copies
        Packing::EncryptedBorrowable { headers_encrypted }
    } else {
        Packing::Borrowable
    }
}

/// Whether `password` passes the RAR5 password check `file` carries (true
/// when it carries none).
fn rar5_password_fits(file: &crate::rarreader::RarFile, password: &[u8]) -> bool {
    match file.crypt {
        Some(c) if c.has_check => {
            let mut check = [0u8; 8];
            check.copy_from_slice(&c.check_value[..8]);
            zipfast::crypto::Rar5PasswordCheck {
                kdf_count: c.kdf_count,
                salt: c.salt,
                check,
            }
            .accepts(&String::from_utf8_lossy(password))
        }
        _ => true,
    }
}

fn le16_at(data: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?))
}

/// Only the first local entry is in the article: it decides when it is the
/// media file, else the packing stays unknown. Stream repair never maps an
/// encrypted zip entry (it takes no password for zip), so one is unborrowable.
fn zip_packing(data: &[u8]) -> Packing {
    let (Some(flags), Some(method), Some(name_len)) =
        (le16_at(data, 6), le16_at(data, 8), le16_at(data, 26))
    else {
        return Packing::Unknown;
    };
    let Some(name) = data.get(30..30 + usize::from(name_len)) else {
        return Packing::Unknown;
    };
    if !is_media_name(name) {
        return Packing::Unknown;
    }
    if method != 0 || flags & 1 != 0 {
        Packing::Unborrowable
    } else {
        Packing::Borrowable
    }
}

/// Whether `name` is a 7-Zip archive or one of its volumes (`x.7z`, `x.7z.001`).
pub fn is_7z_name(name: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(b".7z") {
        return true;
    }
    match lower.iter().rposition(|&b| b == b'.') {
        Some(dot)
            if dot > 3
                && lower[dot + 1..].iter().all(u8::is_ascii_digit)
                && dot + 1 < lower.len() =>
        {
            lower[..dot].ends_with(b".7z")
        }
        _ => false,
    }
}

/// The packaging of a copy, read from the decoded first article of its
/// largest data file (`name` is that file's name in the nzb).
/// `password`: the archive's password, when the nzb carries one.
pub fn packing_of_head(data: &[u8], name: &[u8], password: Option<&[u8]>) -> Packing {
    let password = password.filter(|p| !p.trim_ascii().is_empty());
    let password_known = password.is_some();
    if data.starts_with(b"Rar!\x1a\x07") {
        return rar_packing(data, password);
    }
    if data.starts_with(b"PK\x03\x04") {
        return zip_packing(data);
    }
    if data.starts_with(&[b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c]) {
        // A 7-Zip's headers are at its end, so the head cannot tell how it is
        // packed. A password in the nzb means an encrypted one: stream repair
        // has to decrypt and decompress the whole archive to borrow from it,
        // the slowest donor there is, so it ranks with the unborrowable ones.
        return if password_known {
            Packing::Unborrowable
        } else {
            Packing::Unknown
        };
    }
    // a later piece of a split 7-Zip (x.7z.003) has no signature; a password
    // in the nzb makes it unborrowable all the same
    if password_known && is_7z_name(name) {
        return Packing::Unborrowable;
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

/// How easy a copy is to borrow INTO when it is the primary, lower first (the
/// user's rule, 2026-11-23): bare or split media (0), then a stored archive
/// (1), then a compressed archive, a 7-Zip or an unknown packing (2), then an
/// encrypted one (3: encrypted headers or data, or a 7-Zip whose nzb carries a
/// password). `head_name` is the name of the file whose first article gave
/// `packing`.
pub fn primary_ease(packing: Packing, head_name: &[u8], password_known: bool) -> u8 {
    match packing {
        Packing::HeadersEncrypted | Packing::EncryptedBorrowable { .. } => 3,
        Packing::Unborrowable if password_known && is_7z_name(head_name) => 3,
        Packing::Unborrowable | Packing::Unknown => 2,
        Packing::Borrowable => {
            // borrowable and not an archive: bare media, a raw split, or a
            // media file recognised by its first bytes under another name
            let ext = extension(head_name);
            let archive = matches!(ext.as_slice(), b"rar" | b"zip" | b"7z")
                || (ext.len() == 3
                    && matches!(ext[0], b'r' | b'z')
                    && ext[1..].iter().all(u8::is_ascii_digit));
            u8::from(archive)
        }
    }
}

/// Whether two copies are TWINs, the same post: their par2 sets list the
/// same files by MD5 and length (a near-twin sharing most of a complete list
/// counts too, as for article borrowing) and their data files are split into
/// the same numbers of articles (`a_layout`, `b_layout`: the article count of
/// each data file, sorted), so articles swap one for one. Size alone never
/// makes a twin: a re-upload, or a copy with encrypted headers (a fresh salt
/// every time), has the same file count and total size but different bytes.
/// Without par2 lists two copies are not TWINs.
pub fn twins(
    a: &crate::twin::Listed,
    b: &crate::twin::Listed,
    a_layout: &[u32],
    b_layout: &[u32],
) -> bool {
    !a_layout.is_empty()
        && a_layout == b_layout
        && (crate::twin::kind(a, b).starts_with("twin")
            || crate::twin::kind(b, a).starts_with("twin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_piece_of_a_passworded_split_7z_is_unborrowable() {
        let body = [0x5au8; 64];
        assert_eq!(
            packing_of_head(&body, b"x.7z.003", Some(b"pw")),
            Packing::Unborrowable
        );
        assert_eq!(packing_of_head(&body, b"x.7z", Some(b"pw")), Packing::Unborrowable);
        // without a password a 7-Zip piece stays unknown
        assert_eq!(packing_of_head(&body, b"x.7z.003", None), Packing::Unknown);
    }
    use crate::twin::{FileSig, Listed};

    fn rar4_named(name: &[u8], method: u8, file_flags: u16, main_flags: u16) -> Vec<u8> {
        let mut v = b"Rar!\x1a\x07\x00".to_vec();
        // main header: crc, type 0x73, flags, size 13
        v.extend_from_slice(&[0, 0, 0x73]);
        v.extend_from_slice(&main_flags.to_le_bytes());
        v.extend_from_slice(&13u16.to_le_bytes());
        v.extend_from_slice(&[0; 6]);
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
        // header checksums: the low 16 bits of the CRC32 of each header
        // after its own checksum field
        for (at, len) in [(7, 13), (20, usize::from(size))] {
            let crc = crc32fast::hash(&v[at + 2..at + len]) as u16;
            v[at..at + 2].copy_from_slice(&crc.to_le_bytes());
        }
        v.extend_from_slice(&[0xaa; 64]); // start of the packed data
        v
    }

    fn rar4_with_file(method: u8, file_flags: u16, main_flags: u16) -> Vec<u8> {
        rar4_named(b"movie.mkv", method, file_flags, main_flags)
    }

    #[test]
    fn only_a_labelled_unknown_backup_counts_as_an_alt() {
        let f = unknown_kind_is_labelled_alt;
        // unlabelled: a same-key grab of anything
        assert!(!f(None, None, None));
        assert!(!f(Some(b"unknown"), None, None));
        assert!(!f(Some(b"unknown"), None, Some(b"unverified")));
        // TwinCheck: another container (same video, or not verified yet)
        assert!(f(None, Some(b"container"), None));
        assert!(f(Some(b"unknown"), Some(b"alt"), None));
        // a fleet that found the same video inside
        assert!(f(Some(b"unknown"), None, Some(b"same")));
        // another video, whatever the fleet said
        assert!(!f(Some(b"unknown"), Some(b"other"), Some(b"same")));
    }

    #[test]
    fn rar_without_media_entry_is_unknown() {
        assert_eq!(
            packing_of_head(&rar4_named(b"info.nfo", 0x30, 0, 0), b"a.rar", None),
            Packing::Unknown
        );
    }

    #[test]
    fn locked_only_while_no_password_is_at_hand() {
        assert!(locked_with(true, false, false));
        // its own password, or a pass file to try, unlocks it
        assert!(!locked_with(true, true, false));
        assert!(!locked_with(true, false, true));
        assert!(!locked_with(false, false, false));
        // the stored fact and the password a queued item has now
        // (no test sets UnpackPassFile)
        assert!(locked_params(Some(b"yes"), None));
        assert!(!locked_params(Some(b"yes"), Some(b"pw")));
        assert!(locked_params(Some(b"yes"), Some(b"  ")));
        assert!(!locked_params(None, None));
        assert!(!locked_params(Some(b"no"), None));
    }

    #[test]
    fn rar_store_compressed_and_encrypted() {
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0, 0), b"a.rar", None),
            Packing::Borrowable
        );
        assert_eq!(
            packing_of_head(&rar4_with_file(0x33, 0, 0), b"a.rar", None),
            Packing::Unborrowable
        );
        // encrypted file data, store mode: usable only with the password, and
        // then after the plain borrowable copies
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0x04, 0), b"a.rar", None),
            Packing::Unborrowable
        );
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0x04, 0), b"a.rar", Some(b"pw")),
            Packing::EncryptedBorrowable {
                headers_encrypted: false
            }
        );
        // encrypted headers the password does not open (this one is no
        // real encryption): last, password or not
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0, 0x80), b"a.rar", None),
            Packing::HeadersEncrypted
        );
        assert_eq!(
            packing_of_head(&rar4_with_file(0x30, 0, 0x80), b"a.rar", Some(b"pw")),
            Packing::HeadersEncrypted
        );
        assert_eq!(
            DonorKind::of(false, Packing::HeadersEncrypted),
            DonorKind::Unborrowable
        );
        // a header cut short tells nothing
        assert_eq!(
            packing_of_head(b"Rar!\x1a\x07\x00\x00", b"a.rar", None),
            Packing::Unknown
        );
    }

    fn fixture(dir: &str, name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/testdata")
                .join(dir)
                .join(name),
        )
        .unwrap()
    }

    /// F10: bare or split media, then stored archives, then compressed, then
    /// encrypted.
    #[test]
    fn primary_ease_orders_bare_stored_compressed_encrypted() {
        assert_eq!(primary_ease(Packing::Borrowable, b"movie.mkv", false), 0);
        assert_eq!(primary_ease(Packing::Borrowable, b"movie.mkv.001", false), 0);
        assert_eq!(primary_ease(Packing::Borrowable, b"aGVsbG8", false), 0);
        assert_eq!(primary_ease(Packing::Borrowable, b"movie.part01.rar", false), 1);
        assert_eq!(primary_ease(Packing::Borrowable, b"movie.r00", false), 1);
        assert_eq!(primary_ease(Packing::Borrowable, b"movie.zip", false), 1);
        assert_eq!(primary_ease(Packing::Unborrowable, b"movie.rar", false), 2);
        assert_eq!(primary_ease(Packing::Unknown, b"movie.7z", false), 2);
        assert_eq!(primary_ease(Packing::Unborrowable, b"movie.7z.001", true), 3);
        assert_eq!(primary_ease(Packing::HeadersEncrypted, b"movie.rar", false), 3);
        assert_eq!(
            primary_ease(Packing::EncryptedBorrowable { headers_encrypted: false }, b"movie.rar", true),
            3
        );
    }

    #[test]
    fn header_encrypted_rar_with_its_password_is_borrowable_encrypted() {
        // RAR5 -hp, store mode, password "pw"
        let store = fixture("donorkind", "enchp5.rar");
        let found = packing_of_head(&store, b"a.rar", Some(b"pw"));
        assert_eq!(
            found,
            Packing::EncryptedBorrowable {
                headers_encrypted: true
            }
        );
        assert!(found.headers_encrypted());
        assert_eq!(DonorKind::of(false, found), DonorKind::EncryptedBorrowable);
        // only the first article of a big volume is read: its start is enough
        assert_eq!(packing_of_head(&store[..store.len() / 2], b"a.rar", Some(b"pw")), found);
        // no password, a blank one, or a wrong one: nothing can be told
        assert_eq!(packing_of_head(&store, b"a.rar", None), Packing::HeadersEncrypted);
        assert_eq!(packing_of_head(&store, b"a.rar", Some(b" ")), Packing::HeadersEncrypted);
        assert_eq!(
            packing_of_head(&store, b"a.rar", Some(b"wrong")),
            Packing::HeadersEncrypted
        );
        // only the data encrypted (-p): the file header's check value tells a
        // wrong password
        let data = fixture("donorkind", "encdata5.rar");
        assert_eq!(
            packing_of_head(&data, b"a.rar", Some(b"pw")),
            Packing::EncryptedBorrowable {
                headers_encrypted: false
            }
        );
        assert_eq!(packing_of_head(&data, b"a.rar", Some(b"wrong")), Packing::Unborrowable);
        assert_eq!(packing_of_head(&data, b"a.rar", None), Packing::Unborrowable);
        // the media inside is compressed: still unborrowable, password or not
        let comp = fixture("donorkind", "enchp5comp.rar");
        assert_eq!(packing_of_head(&comp, b"a.rar", Some(b"pw")), Packing::HeadersEncrypted);
        assert!(Packing::HeadersEncrypted.headers_encrypted());
        assert!(!Packing::EncryptedBorrowable {
            headers_encrypted: false
        }
        .headers_encrypted());
    }

    #[test]
    fn kinds_order_and_round_trip() {
        use DonorKind::*;
        assert!(Twin < Borrowable);
        assert!(Borrowable < EncryptedBorrowable);
        assert!(EncryptedBorrowable < Unknown);
        assert!(Unknown < Unborrowable);
        for k in [Twin, Borrowable, EncryptedBorrowable, Unknown, Unborrowable] {
            assert_eq!(DonorKind::from_u8(k as u8), k);
            if k != Twin {
                assert_eq!(DonorKind::from_param(Some(k.param())), k);
            }
        }
    }

    #[test]
    fn zip_7z_and_bare() {
        let zip_entry = |name: &[u8], flags: u8, method: u8| {
            let mut z = b"PK\x03\x04\x14\x00".to_vec();
            z.extend_from_slice(&[flags, 0, method, 0]);
            z.extend_from_slice(&[0; 16]); // time, date, crc, sizes
            z.extend_from_slice(&(name.len() as u16).to_le_bytes());
            z.extend_from_slice(&[0, 0]); // extra length
            z.extend_from_slice(name);
            z.extend_from_slice(&[0xaa; 32]);
            z
        };
        let z = zip_entry(b"movie.mkv", 0, 0);
        assert_eq!(packing_of_head(&z, b"a.zip", None), Packing::Borrowable);
        let z = zip_entry(b"movie.mkv", 0, 8); // deflate
        assert_eq!(packing_of_head(&z, b"a.zip", None), Packing::Unborrowable);
        // encrypted entries are never mapped, password or not
        let z = zip_entry(b"movie.mkv", 1, 0);
        assert_eq!(packing_of_head(&z, b"a.zip", None), Packing::Unborrowable);
        assert_eq!(packing_of_head(&z, b"a.zip", Some(b"pw")), Packing::Unborrowable);
        // a stored nfo first says nothing about the movie behind it
        let z = zip_entry(b"info.nfo", 0, 0);
        assert_eq!(packing_of_head(&z, b"a.zip", None), Packing::Unknown);
        let z = zip_entry(b"movie-sample.mkv", 0, 0);
        assert_eq!(packing_of_head(&z, b"a.zip", None), Packing::Unknown);
        assert_eq!(
            packing_of_head(&[b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c, 0, 4], b"a.7z", None),
            Packing::Unknown
        );
        // an nzb password on a 7-Zip: encrypted, ranked after every other ALT
        assert_eq!(
            packing_of_head(
                &[b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c, 0, 4],
                b"a.7z.001", Some(b"pw")),
            Packing::Unborrowable
        );
        assert_eq!(
            packing_of_head(b"anything", b"Movie.2020.mkv", None),
            Packing::Borrowable
        );
        assert_eq!(
            packing_of_head(&[0x1a, 0x45, 0xdf, 0xa3, 1, 2], b"abc123", None),
            Packing::Borrowable
        );
        assert_eq!(
            packing_of_head(b"anything", b"Movie.2020.mkv.001", None),
            Packing::Borrowable
        );
        assert_eq!(
            packing_of_head(b"anything", b"setup.exe", None),
            Packing::Unknown
        );
    }

    #[test]
    fn seven_zip_names() {
        assert!(is_7z_name(b"a.7z"));
        assert!(is_7z_name(b"X.7Z.001"));
        assert!(is_7z_name(b"rel.7z.123"));
        assert!(!is_7z_name(b"a.rar"));
        assert!(!is_7z_name(b"movie.mkv"));
        assert!(!is_7z_name(b"a.7z.par2"));
        assert!(!is_7z_name(b"a.001"));
        assert!(!is_7z_name(b".7z."));
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
                .map(|&(name, length, md5)| {
                    // a test label for the MD5, zero-padded
                    let mut label = [0u8; 16];
                    label[..md5.len()].copy_from_slice(md5.as_bytes());
                    FileSig {
                        name: name.into(),
                        length,
                        md5: label,
                    }
                })
                .collect(),
            complete,
            print: String::new(),
        }
    }

    #[test]
    fn twins_need_matching_par2_and_articles() {
        let a = listed(&[("a.r00", 10, "m1"), ("a.r01", 10, "m2")], true);
        let b = listed(&[("b.r00", 10, "m1"), ("b.r01", 10, "m2")], true);
        let c = listed(&[("c.r00", 10, "x1"), ("c.r01", 10, "x2")], true);
        let l = [3u32, 3];
        assert!(twins(&a, &b, &l, &l));
        // same par2 hashes, other article split: an ALT, not swappable
        assert!(!twins(&a, &b, &l, &[2, 4]));
        assert!(!twins(&a, &b, &[], &[]));
        // same names and sizes, other bytes (a re-upload with encrypted
        // headers): not twins
        assert!(!twins(&a, &c, &l, &l));
        // a partial list sharing most files of a complete one: a near-twin
        let three = listed(
            &[
                ("t.r00", 10, "m1"),
                ("t.r01", 10, "m2"),
                ("t.r02", 10, "m3"),
            ],
            true,
        );
        let part = listed(&[("p.r00", 10, "m1"), ("p.r01", 10, "m2")], false);
        assert!(twins(&three, &part, &l, &l));
        assert!(twins(&part, &three, &l, &l));
        // without par2 lists nothing is a twin, whatever the sizes
        let none = listed(&[], false);
        assert!(!twins(&none, &c, &l, &l));
        assert!(!twins(&none, &none, &l, &l));
    }
}
