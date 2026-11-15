//! RarVolume's header reading (RarReader.cpp) on top of zipfast's single-volume
//! map (`zipfast::map::VolumeMapBuilder`): the version, naming and volume flags
//! of one RAR3 or RAR5 volume and the files it holds with their sizes, times,
//! split flags, methods, data offsets and encryption records. zipfast parses
//! and decrypts the headers; this module only reads the volume's bytes for it
//! from a `Source` and reports the result in the C++ reader's shapes.

use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use zipfast::map::{
    CipherParams, MapError, MapMethod, MapOptions, VolumeEnd, VolumeMap, VolumeMapBuilder,
};
use zipfast::progressive::{
    ProgressiveResult, RangeRead, RangeSource, SourceId, SourceInfo, SourceLength,
};

/// The content a volume is read from (ContentSource).
pub trait Source {
    fn size(&mut self) -> i64;
    /// `buf.len()` bytes at `offset`, or false
    fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool;
}

/// Receives files at the same commit points as the C++ reader. A failed
/// append stops parsing; a successful seek updates the last file's offset.
pub trait Files {
    fn add(&mut self, file: &RarFile) -> bool;
    fn set_data_offset(&mut self, position: i64);
}

/// RarFile::Rar5Crypt
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rar5Crypt {
    pub version: u64,
    pub flags: u64,
    pub kdf_count: u8,
    pub salt: [u8; 16],
    pub iv: [u8; 16],
    pub check_value: [u8; 12],
    pub has_check: bool,
}

/// RarFile
#[derive(Clone, Debug, PartialEq)]
pub struct RarFile {
    /// as the C++ held it: up to the first NUL of the stored name
    pub filename: Vec<u8>,
    pub time: u32,
    pub attr: u32,
    pub size: i64,
    pub split_before: bool,
    pub split_after: bool,
    pub method: u32,
    pub stored: bool,
    pub packed_size: i64,
    pub data_offset: i64,
    pub encrypted_data: bool,
    pub has_salt: bool,
    pub salt: [u8; 8],
    pub crypt: Option<Rar5Crypt>,
}

impl Default for RarFile {
    fn default() -> Self {
        RarFile {
            filename: Vec::new(),
            time: 0,
            attr: 0,
            size: 0,
            split_before: false,
            split_after: false,
            method: 0,
            stored: false,
            packed_size: 0,
            data_offset: -1,
            encrypted_data: false,
            has_salt: false,
            salt: [0; 8],
            crypt: None,
        }
    }
}

/// The volume state RarVolume keeps (read and updated by a parse).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Volume {
    pub version: i32,
    pub volume_no: u32,
    pub new_naming: bool,
    pub has_next_volume: bool,
    pub multi_volume: bool,
    pub encrypted: bool,
    pub has_password: bool,
}

/// DetectRarVersion: the signatures compared as C strings (strcmp)
fn detect_version(source: &mut dyn Source, size: i64) -> i32 {
    const RAR3: &[u8] = b"Rar!\x1a\x07\x00";
    const RAR5: &[u8] = b"Rar!\x1a\x07\x01\x00";
    let mut sig = [0u8; 8];
    if size < 8 || !source.read(0, &mut sig) {
        return 0;
    }
    // strcmp(signature, read): equal up to and including the signature's NUL
    let strcmp_eq = |s: &[u8]| s.iter().zip(sig.iter()).all(|(a, b)| a == b);
    if strcmp_eq(RAR5) {
        5
    } else if strcmp_eq(RAR3) {
        3
    } else {
        0
    }
}

/// What the map's reads and the volume's bytes share: zipfast asks through
/// `RangeSource::read_at`, which answers `NotYet` until `walk` has read the
/// asked range from the borrowed `Source`.
struct Shared {
    /// where the volume ends for the map: the source's size, or the start of
    /// the first range the source could not read
    len: u64,
    want: Option<(u64, usize)>,
    have: (u64, Vec<u8>),
    /// the furthest offset the map asked for
    furthest: u64,
}

struct Bridge(Arc<Mutex<Shared>>);

impl RangeSource for Bridge {
    fn info(&self) -> SourceInfo {
        let len = self.0.lock().map_or(0, |s| s.len);
        SourceInfo {
            id: SourceId(0),
            generation: 0,
            extent: len,
            length: SourceLength::Final(len),
        }
    }

    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        _: &mut Context<'_>,
    ) -> ProgressiveResult<RangeRead> {
        let mut s = self.0.lock().unwrap_or_else(|e| e.into_inner());
        s.furthest = s.furthest.max(offset);
        let (at, data) = &s.have;
        if let Some(rel) = offset
            .checked_sub(*at)
            .and_then(|r| usize::try_from(r).ok())
        {
            if rel < data.len() {
                let count = buf.len().min(data.len() - rel);
                buf[..count].copy_from_slice(&data[rel..rel + count]);
                return Ok(RangeRead::Filled { count });
            }
        }
        s.want = Some((offset, buf.len()));
        Ok(RangeRead::NotYet)
    }

    fn backing_files(&self) -> &[std::fs::File] {
        &[]
    }
}

/// The longest prefix of `len` bytes at `offset` that `source` reads, found by
/// halving: a content source fails a whole read that runs into a missing
/// article, where the C++ reader, reading field by field, got up to it.
fn read_prefix(source: &mut dyn Source, offset: u64, len: usize) -> Vec<u8> {
    let Ok(at) = i64::try_from(offset) else {
        return Vec::new();
    };
    let mut want = len;
    while want > 0 {
        let mut buf = vec![0u8; want];
        if source.read(at, &mut buf) {
            return buf;
        }
        want /= 2;
    }
    Vec::new()
}

/// Maps the volume behind `source`, ending it at `cap` when given.
fn walk(
    source: &mut dyn Source,
    size: u64,
    password: Option<&str>,
    cap: Option<u64>,
) -> (Result<VolumeMap, MapError>, u64) {
    let shared = Arc::new(Mutex::new(Shared {
        len: cap.map_or(size, |c| c.min(size)),
        want: None,
        have: (0, Vec::new()),
        furthest: 0,
    }));
    let options = MapOptions::new("volume.rar").password(password);
    let mut builder = VolumeMapBuilder::new(Box::new(Bridge(shared.clone())), options);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(result) = builder.poll_map(&mut cx) {
            let furthest = shared.lock().map_or(0, |s| s.furthest);
            return (result, furthest);
        }
        let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
        let Some((offset, len)) = s.want.take() else {
            return (Err(MapError::AlreadyFinished), s.furthest);
        };
        let len = len.min(usize::try_from(s.len.saturating_sub(offset)).unwrap_or(usize::MAX));
        let data = read_prefix(source, offset, len);
        if data.is_empty() {
            // the volume ends here for the map: what it read before is kept
            s.len = s.len.min(offset);
        }
        s.have = (offset, data);
    }
}

/// A unix time as a DOS date and time (RAR3 headers store those; zipfast
/// reports them as unix times read as UTC).
fn dos_time(unix: i64) -> u32 {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // civil date from days since 1970 (proleptic Gregorian)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if !(1980..=2107).contains(&year) {
        return 0;
    }
    (((year - 1980) as u32) << 25)
        | ((month as u32) << 21)
        | ((day as u32) << 16)
        | (((secs / 3600) as u32) << 11)
        | ((((secs / 60) % 60) as u32) << 5)
        | (((secs % 60) / 2) as u32)
}

fn rar_file(m: &zipfast::map::VolumeMember, rar5: bool) -> RarFile {
    let mut filename = m.raw_name.clone();
    filename.truncate(
        filename
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(filename.len()),
    );
    let (method, stored) = match m.method {
        MapMethod::Stored => (if rar5 { 0 } else { 0x30 }, true),
        MapMethod::Compressed { id } => (u32::from(id), false),
        _ => (u32::MAX, false),
    };
    let time = m.mtime.map_or(
        0,
        |(secs, _)| if rar5 { secs as u32 } else { dos_time(secs) },
    );
    let mut f = RarFile {
        filename,
        time,
        attr: m.attributes as u32,
        size: m.unpacked_size.map_or(0, |s| s as i64),
        split_before: m.split_before,
        split_after: m.split_after,
        method,
        stored,
        packed_size: m.packed_size as i64,
        data_offset: -1,
        encrypted_data: m.encrypted,
        ..RarFile::default()
    };
    match &m.cipher {
        Some(CipherParams::Rar5 {
            kdf_count,
            salt,
            iv,
            check,
            keyed_checks,
        }) => {
            let mut check_value = [0u8; 12];
            if let Some((value, sum)) = check {
                check_value[..8].copy_from_slice(value);
                check_value[8..].copy_from_slice(sum);
            }
            f.crypt = Some(Rar5Crypt {
                version: 0,
                flags: u64::from(check.is_some()) | (u64::from(*keyed_checks) << 1),
                kdf_count: *kdf_count,
                salt: *salt,
                iv: iv.unwrap_or_default(),
                check_value,
                has_check: check.is_some(),
            });
        }
        Some(
            CipherParams::Rar4 { salt }
            | CipherParams::Rar4Legacy {
                salt: Some(salt), ..
            },
        ) => {
            f.has_salt = true;
            f.salt = *salt;
        }
        _ => {}
    }
    f
}

/// RarVolume::ReadFrom: reads the volume behind `source` into `volume` and
/// `files`; true when the volume was read to its end header. Encrypted headers
/// are read with `password` when `volume.has_password` is set. A volume cut
/// short, or damaged past some file headers, still reports those files.
pub fn read_from(
    volume: &mut Volume,
    source: &mut dyn Source,
    password: Option<&[u8]>,
    files: &mut dyn Files,
) -> bool {
    let size = source.size();
    volume.version = detect_version(source, size);
    if volume.version == 0 {
        return false;
    }
    let size = size as u64;
    let password = password
        .filter(|p| volume.has_password && !p.is_empty())
        .map(String::from_utf8_lossy);
    let password = password.as_deref();
    let map = match walk(source, size, password, None) {
        (Ok(map), _) => map,
        // A damaged header: keep what came before it, as the C++ reader did,
        // by ending the volume where the map last read.
        (Err(MapError::Malformed { .. } | MapError::Unsupported { .. }), furthest)
            if furthest > 0 =>
        {
            match walk(source, size, password, Some(furthest)) {
                (Ok(map), _) => map,
                _ => return false,
            }
        }
        _ => return false,
    };
    if map.header_encrypted {
        volume.encrypted = true;
    }
    if map.main_read {
        volume.new_naming = map.new_naming;
        volume.multi_volume = map.multi_volume;
        if let Some(no) = map.volume_number {
            volume.volume_no = no;
        }
    }
    let complete = map.end == VolumeEnd::Complete;
    if complete {
        volume.has_next_volume = map.has_next_volume;
    }
    let rar5 = volume.version == 5;
    for m in &map.members {
        if !files.add(&rar_file(m, rar5)) {
            return false;
        }
        files.set_data_offset(m.packed.end as i64);
    }
    complete
}

impl Files for Vec<RarFile> {
    fn add(&mut self, file: &RarFile) -> bool {
        self.push(file.clone());
        true
    }
    fn set_data_offset(&mut self, position: i64) {
        if let Some(last) = self.last_mut() {
            last.data_offset = position.wrapping_sub(last.packed_size);
        }
    }
}

const FILE_SOURCE_BUF_SIZE: usize = 8192;

/// Disk-backed `Source` sized through its open file handle (`RarVolume::Read`),
/// with an in-memory read window so sequential byte reads (`read_v`) and nearby
/// header seeks are satisfied from memory without per-field `lseek`/`read` syscalls.
pub struct FileSource {
    file: std::fs::File,
    size: i64,
    file_pos: i64,
    buf: [u8; FILE_SOURCE_BUF_SIZE],
    buf_start: i64,
    buf_len: usize,
}

impl FileSource {
    pub fn open(path: &[u8]) -> Option<Self> {
        use std::io::{Seek, SeekFrom};
        let mut file = std::fs::File::open(crate::fsops::path(path)).ok()?;
        let size = file.seek(SeekFrom::End(0)).ok()? as i64;
        if size < 0 {
            return None;
        }
        file.seek(SeekFrom::Start(0)).ok()?;
        Some(Self {
            file,
            size,
            file_pos: 0,
            buf: [0u8; FILE_SOURCE_BUF_SIZE],
            buf_start: 0,
            buf_len: 0,
        })
    }
}

impl Source for FileSource {
    fn size(&mut self) -> i64 {
        self.size
    }
    fn read(&mut self, offset: i64, out: &mut [u8]) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        if offset < 0 || offset > self.size || (out.len() as i64) > self.size - offset {
            return false;
        }
        if out.is_empty() {
            return true;
        }
        let end = offset + out.len() as i64;
        let buf_end = self.buf_start + self.buf_len as i64;
        if offset >= self.buf_start && end <= buf_end {
            let rel = (offset - self.buf_start) as usize;
            out.copy_from_slice(&self.buf[rel..rel + out.len()]);
            return true;
        }
        if out.len() >= FILE_SOURCE_BUF_SIZE {
            if self.file_pos != offset && self.file.seek(SeekFrom::Start(offset as u64)).is_err() {
                self.file_pos = -1;
                return false;
            }
            if self.file.read_exact(out).is_err() {
                self.file_pos = -1;
                return false;
            }
            self.file_pos = end;
            return true;
        }
        if self.file_pos != offset && self.file.seek(SeekFrom::Start(offset as u64)).is_err() {
            self.file_pos = -1;
            return false;
        }
        let to_read = FILE_SOURCE_BUF_SIZE.min((self.size - offset) as usize);
        if self.file.read_exact(&mut self.buf[..to_read]).is_err() {
            self.file_pos = -1;
            self.buf_len = 0;
            return false;
        }
        self.buf_start = offset;
        self.buf_len = to_read;
        self.file_pos = offset + to_read as i64;
        out.copy_from_slice(&self.buf[..out.len()]);
        true
    }
}

/// `RarVolume::Read` (`RarReader.cpp`): opens `path`, sizes through the open handle,
/// and reads RAR3/RAR5 headers, decrypting encrypted ones only when `tls_enabled` is true.
pub fn read_volume_file(
    path: &[u8],
    password: &[u8],
    tls_enabled: bool,
) -> (bool, Volume, Vec<RarFile>) {
    let mut vol = Volume {
        has_password: !password.is_empty(),
        ..Volume::default()
    };
    let mut files = Vec::new();
    let Some(mut source) = FileSource::open(path) else {
        return (false, vol, files);
    };
    // without TLS there is no AES: encrypted headers stay unread
    let password = tls_enabled.then_some(password);
    let ok = read_from(&mut vol, &mut source, password, &mut files);
    (ok, vol, files)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Bytes(Vec<u8>);
    impl Source for Bytes {
        fn size(&mut self) -> i64 {
            self.0.len() as i64
        }
        fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool {
            let Some(bytes) = self.0.get(offset as usize..offset as usize + buf.len()) else {
                return false;
            };
            buf.copy_from_slice(bytes);
            true
        }
    }

    #[test]
    fn rar_reader_full_suite_port_from_cpp() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let testdata_dir = manifest_dir
            .parent()
            .unwrap()
            .join("tests/testdata/rarrenamer");

        let read_vol = |name: &str, password: Option<&[u8]>| -> (bool, Volume, Vec<RarFile>, i64) {
            let path = testdata_dir.join(name);
            let bytes = std::fs::read(&path).unwrap();
            let size = bytes.len() as i64;
            let mut source = Bytes(bytes);
            let mut volume = Volume {
                has_password: password.is_some(),
                ..Volume::default()
            };
            let mut files = Vec::new();
            let ok = read_from(&mut volume, &mut source, password, &mut files);
            (ok, volume, files, size)
        };

        // 1. Rar3Test
        for (i, name) in [
            "testfile3.part01.rar",
            "testfile3.part02.rar",
            "testfile3.part03.rar",
        ]
        .iter()
        .enumerate()
        {
            let (ok, vol, _, _) = read_vol(name, None);
            assert!(ok);
            assert_eq!(vol.version, 3);
            assert!(vol.multi_volume);
            assert!(vol.new_naming);
            assert_eq!(vol.volume_no, i as u32);
        }

        // 2. Rar5OverlongVintTest
        {
            let mut bytes = vec![b'R', b'a', b'r', b'!', 0x1a, 0x07, 0x01, 0x00, 0, 0, 0, 0];
            bytes.extend([0xffu8; 16]);
            bytes.push(0x01);
            bytes.extend([0u8; 64]);
            let mut source = Bytes(bytes);
            let mut vol = Volume::default();
            let mut files = Vec::new();
            assert!(!read_from(&mut vol, &mut source, None, &mut files));
        }

        // 3. Rar3ZeroAdvanceTest
        {
            let bytes = vec![
                b'R', b'a', b'r', b'!', 0x1a, 0x07, 0x00, 0x00, 0x00, 0x73, 0x00, 0x00, 0x00, 0x00,
            ];
            let mut source = Bytes(bytes);
            let mut vol = Volume::default();
            let mut files = Vec::new();
            assert!(!read_from(&mut vol, &mut source, None, &mut files));
        }

        // 4. Rar5Test
        for (i, name) in [
            "testfile5.part01.rar",
            "testfile5.part02.rar",
            "testfile5.part03.rar",
        ]
        .iter()
        .enumerate()
        {
            let (ok, vol, _, _) = read_vol(name, None);
            assert!(ok);
            assert_eq!(vol.version, 5);
            assert!(vol.multi_volume);
            assert!(vol.new_naming);
            assert_eq!(vol.volume_no, i as u32);
        }

        // 5. Rar3OldNamingTest
        for (i, name) in [
            "testfile3oldnam.rar",
            "testfile3oldnam.r00",
            "testfile3oldnam.r01",
        ]
        .iter()
        .enumerate()
        {
            let (ok, vol, _, _) = read_vol(name, None);
            assert!(ok);
            assert_eq!(vol.version, 3);
            assert!(vol.multi_volume);
            assert!(!vol.new_naming);
            assert_eq!(vol.volume_no, i as u32);
        }

        // 6. Rar3EncryptedDataTest
        for (i, name) in [
            "testfile3encdata.part01.rar",
            "testfile3encdata.part02.rar",
            "testfile3encdata.part03.rar",
        ]
        .iter()
        .enumerate()
        {
            let (ok, vol, _, _) = read_vol(name, None);
            assert!(ok);
            assert_eq!(vol.version, 3);
            assert!(vol.multi_volume);
            assert!(vol.new_naming);
            assert_eq!(vol.volume_no, i as u32);
        }

        // 7. Rar5EncryptedDataTest
        for (i, name) in [
            "testfile5encdata.part01.rar",
            "testfile5encdata.part02.rar",
            "testfile5encdata.part03.rar",
        ]
        .iter()
        .enumerate()
        {
            let (ok, vol, _, _) = read_vol(name, None);
            assert!(ok);
            assert_eq!(vol.version, 5);
            assert!(vol.multi_volume);
            assert!(vol.new_naming);
            assert_eq!(vol.volume_no, i as u32);
        }

        // 8. Rar3EcryptedNamesTest (no password)
        {
            let (ok, vol, _, _) = read_vol("testfile3encnam.part01.rar", None);
            assert!(!ok);
            assert_eq!(vol.version, 3);
            assert!(vol.encrypted);
        }

        // 9. Rar3EncryptedNamesKdfTest (with password "123")
        for (i, name) in [
            "testfile3encnam.part01.rar",
            "testfile3encnam.part02.rar",
            "testfile3encnam.part03.rar",
        ]
        .iter()
        .enumerate()
        {
            let (ok, vol, files, _) = read_vol(name, Some(b"123"));
            assert!(ok);
            assert_eq!(vol.version, 3);
            assert!(vol.multi_volume);
            assert!(vol.new_naming);
            assert_eq!(vol.volume_no, i as u32);
            assert!(vol.encrypted);
            assert!(!files.is_empty());
            assert_eq!(files[0].filename, b"testfile3encnam.dat");
        }

        // 10. Rar5EncryptedNamesKdfTest (with password "123" and without password)
        for (i, name) in [
            "testfile5encnam.part01.rar",
            "testfile5encnam.part02.rar",
            "testfile5encnam.part03.rar",
        ]
        .iter()
        .enumerate()
        {
            let (ok, vol, files, _) = read_vol(name, Some(b"123"));
            assert!(ok);
            assert_eq!(vol.version, 5);
            assert!(vol.multi_volume);
            assert!(vol.new_naming);
            assert_eq!(vol.volume_no, i as u32);
            assert!(vol.encrypted);
            assert!(!files.is_empty());
            assert_eq!(files[0].filename, b"testfile5encnam.dat");
        }
        {
            let (ok, vol, _, _) = read_vol("testfile5encnam.part01.rar", None);
            assert!(!ok);
            assert_eq!(vol.version, 5);
            assert!(vol.encrypted);
        }

        // 11. RarStoreFieldsTest
        {
            let (ok, _, files, vol_size) = read_vol("testfile3.part01.rar", None);
            assert!(ok);
            let inner = &files[0];
            assert!(inner.packed_size > 0);
            assert!(inner.data_offset > 0);
            assert!(inner.data_offset + inner.packed_size <= vol_size);
            assert_ne!(inner.method, 0);
            assert!(!inner.encrypted_data);
        }
        {
            let (ok, _, files, vol_size) = read_vol("testfile5.part01.rar", None);
            assert!(ok);
            let inner = &files[0];
            assert!(inner.packed_size > 0);
            assert!(inner.data_offset > 0);
            assert!(inner.data_offset + inner.packed_size <= vol_size);
            assert!(!inner.encrypted_data);
        }
        {
            let (ok, _, files, _) = read_vol("testfile3encdata.part01.rar", None);
            assert!(ok);
            assert!(files[0].encrypted_data);
        }
        {
            let (ok, _, files, _) = read_vol("testfile5encdata.part01.rar", None);
            assert!(ok);
            assert!(files[0].encrypted_data);
        }
    }
}
