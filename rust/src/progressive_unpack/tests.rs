use super::*;

thread_local! {
    /// Declares complete sets at session start whatever their estimated cost, and caps
    /// the declared session's protection path bytes so it hits a limit.
    pub(super) static FORCE_DECLARE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
use std::sync::atomic::{AtomicBool, AtomicUsize};

// ---- RAR5 fixture builder (stored members, valid header and data CRCs) ----

fn vint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn block(kind: u64, flags: u64, fields: &[u8], data: &[u8], data_size_field: bool) -> Vec<u8> {
    let mut flags = flags;
    if data_size_field {
        flags |= 2;
    }
    let mut body = Vec::new();
    vint(kind, &mut body);
    vint(flags, &mut body);
    if data_size_field {
        vint(data.len() as u64, &mut body);
    }
    body.extend_from_slice(fields);
    let mut sized = Vec::new();
    vint(body.len() as u64, &mut sized);
    sized.extend_from_slice(&body);
    let mut out = crc32fast::hash(&sized).to_le_bytes().to_vec();
    out.extend_from_slice(&sized);
    out.extend_from_slice(data);
    out
}

thread_local! {
    /// Host OS and attributes `file_block` writes: Unix 0644 unless a test changes them.
    static RAR5_HOST: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((1, 0o644)) };
}

fn file_block(name: &str, size: u64, part: &[u8], crc: u32, before: bool, after: bool) -> Vec<u8> {
    let (host, attributes) = RAR5_HOST.with(|h| h.get());
    let mut fields = Vec::new();
    vint(0x4 | 0x2, &mut fields); // CRC present, mtime present
    vint(size, &mut fields);
    vint(attributes, &mut fields);
    fields.extend_from_slice(&1_700_000_000u32.to_le_bytes());
    fields.extend_from_slice(&crc.to_le_bytes());
    vint(0, &mut fields); // stored
    vint(host, &mut fields); // 1 = Unix, 0 = Windows
    vint(name.len() as u64, &mut fields);
    fields.extend_from_slice(name.as_bytes());
    let mut flags = 0;
    if before {
        flags |= 0x8;
    }
    if after {
        flags |= 0x10;
    }
    block(2, flags, &fields, part, true)
}

fn main_block(volume: bool, number: Option<u64>) -> Vec<u8> {
    let mut fields = Vec::new();
    let mut flags = 0;
    if volume {
        flags |= 1;
    }
    if number.is_some() {
        flags |= 2;
    }
    vint(flags, &mut fields);
    if let Some(n) = number {
        vint(n, &mut fields);
    }
    block(1, 0, &fields, &[], false)
}

fn end_block(more: bool) -> Vec<u8> {
    let mut fields = Vec::new();
    vint(u64::from(more), &mut fields);
    block(5, 0, &fields, &[], false)
}

const SIG: &[u8] = b"Rar!\x1a\x07\x01\x00";

/// One stored file cut into volumes of `chunk` data bytes each.
fn rar5_volumes(name: &str, data: &[u8], chunk: usize) -> Vec<Vec<u8>> {
    let parts: Vec<&[u8]> = data.chunks(chunk).collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let mut out = SIG.to_vec();
            let multi = n > 1;
            out.extend(main_block(
                multi,
                if i == 0 { None } else { Some(i as u64) },
            ));
            let last = i + 1 == n;
            let crc = if last {
                crc32fast::hash(data)
            } else {
                crc32fast::hash(part)
            };
            out.extend(file_block(name, data.len() as u64, part, crc, i > 0, !last));
            out.extend(end_block(!last));
            out
        })
        .collect()
}

fn volume_name(n: usize, count: usize) -> String {
    if count == 1 {
        "set.rar".into()
    } else {
        format!("set.part{:02}.rar", n + 1)
    }
}

/// Data offset of the payload of volume `i` (header bytes before it).
fn payload_offset(vol: &[u8], part_len: usize) -> usize {
    let end = end_block(false).len();
    vol.len() - end - part_len
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i as u64).wrapping_mul(2654435761) >> 7) as u8)
        .collect()
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "rg-progressive-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("dl")).unwrap();
        std::fs::create_dir_all(p.join("out")).unwrap();
        Self(p)
    }
    fn dl(&self) -> PathBuf {
        self.0.join("dl")
    }
    fn out(&self) -> PathBuf {
        self.0.join("out")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_set(dir: &Path, vols: &[Vec<u8>]) -> PathBuf {
    for (i, v) in vols.iter().enumerate() {
        std::fs::write(dir.join(volume_name(i, vols.len())), v).unwrap();
    }
    dir.join(volume_name(0, vols.len()))
}

fn run(
    first: &Path,
    out: &Path,
    feed: Box<dyn VolumeFeed>,
) -> (Result<zipfast::extract::Summary, Error>, Vec<String>) {
    run_with(first, out, feed, Overwrite::Always)
}

fn run_with(
    first: &Path,
    out: &Path,
    feed: Box<dyn VolumeFeed>,
    overwrite: Overwrite,
) -> (Result<zipfast::extract::Summary, Error>, Vec<String>) {
    let mut log = Vec::new();
    let req = Request {
        first_volume: first,
        dest: out,
        password: "",
        overwrite,
        on_wait: None,
    };
    let r = extract(
        &req,
        feed,
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    (r, log)
}

fn expect_ok(
    r: Result<zipfast::extract::Summary, Error>,
    log: &[String],
) -> zipfast::extract::Summary {
    match r {
        Ok(s) => s,
        Err(Error::Failed(f)) => panic!("failed: {f}\nlog: {log:#?}"),
        Err(Error::Unsupported(u)) => panic!("unsupported: {u:?}\nlog: {log:#?}"),
    }
}

#[test]
fn complete_single_volume_set_extracts() {
    let t = TestDir::new("single");
    let data = pattern(300_000);
    let first = write_set(&t.dl(), &rar5_volumes("movie.mkv", &data, 1 << 30));
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    let s = expect_ok(r, &log);
    assert_eq!(s.files, 1);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    // Published by zipfast itself (not copied): the archive's mtime is restored.
    let mtime = std::fs::metadata(t.out().join("movie.mkv"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(mtime, 1_700_000_000);
    assert!(log.iter().any(|l| l == "Unrar: All OK"), "{log:?}");
    assert!(
        log.iter().any(|l| l == "Unrar: Extracting  movie.mkv"),
        "{log:?}"
    );
}

#[test]
fn complete_multi_volume_set_extracts() {
    let t = TestDir::new("multi");
    let data = pattern(1_000_000);
    let first = write_set(&t.dl(), &rar5_volumes("movie.mkv", &data, 100_000));
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("Unrar: Extracting from"))
            .count(),
        10,
        "{log:?}"
    );
}

#[test]
fn missing_volume_is_typed() {
    let t = TestDir::new("missing");
    let data = pattern(400_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    std::fs::remove_file(t.dl().join(volume_name(2, vols.len()))).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    match r {
        Err(Error::Failed(f)) => assert!(f.is_damage(), "{f:?} {log:?}"),
        other => panic!(
            "expected a missing-volume failure, got ok={}",
            other.is_ok()
        ),
    }
    assert!(!t.out().join("movie.mkv").exists());
}

/// Volumes become available one at a time, the session parks in between.
struct Trickle {
    released: Arc<AtomicUsize>,
    names: Vec<PathBuf>,
    asked: Arc<AtomicUsize>,
}

impl VolumeFeed for Trickle {
    fn volume(&mut self, path: &Path) -> VolumeView {
        self.asked.fetch_add(1, Ordering::Relaxed);
        let idx = self.names.iter().position(|n| n == path);
        match idx {
            Some(i) if i < self.released.load(Ordering::Acquire) => {
                VolumeView::Ready { holes: Vec::new() }
            }
            Some(_) => VolumeView::NotYet,
            None => VolumeView::Missing,
        }
    }
}

#[test]
fn volumes_arriving_one_at_a_time() {
    let t = TestDir::new("trickle");
    let data = pattern(500_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let names: Vec<PathBuf> = (0..vols.len())
        .map(|i| t.dl().join(volume_name(i, vols.len())))
        .collect();
    let released = Arc::new(AtomicUsize::new(1));
    let waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
    let feed = Trickle {
        released: released.clone(),
        names,
        asked: Arc::new(AtomicUsize::new(0)),
    };
    let rel = released.clone();
    let wk = waker.clone();
    let n = vols.len();
    let releaser = std::thread::spawn(move || {
        for k in 2..=n {
            std::thread::sleep(Duration::from_millis(30));
            rel.store(k, Ordering::Release);
            if let Some(w) = wk.lock().unwrap().take() {
                w.wake();
            }
        }
    });
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    releaser.join().unwrap();
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

/// A damaged volume is handed over with holes; after the "repair" (the original bytes put
/// back into the volume file) the output is patched from the repaired volume.
struct Damaged {
    holes: Vec<(PathBuf, Vec<(u64, u64)>)>,
    repair: Vec<(PathBuf, Vec<u8>)>,
    handed: bool,
}

impl VolumeFeed for Damaged {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if !path.is_file() {
            return VolumeView::Missing;
        }
        let holes = self
            .holes
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, h)| h.clone())
            .unwrap_or_default();
        self.handed |= !holes.is_empty();
        VolumeView::Ready { holes }
    }
    fn wait_repaired(&mut self, _is_cancelled: &mut dyn FnMut() -> bool) -> bool {
        for (p, bytes) in &self.repair {
            std::fs::write(p, bytes).unwrap();
        }
        true
    }
}

#[test]
fn holes_are_skipped_and_patched_from_the_repaired_volume() {
    let t = TestDir::new("holes");
    let data = pattern(600_000);
    let chunk = 100_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    // Zero out two "articles" in volume 3 and one straddling into volume 4's payload start.
    let v3 = t.dl().join(volume_name(2, vols.len()));
    let v4 = t.dl().join(volume_name(3, vols.len()));
    let p3 = payload_offset(&vols[2], chunk) as u64;
    let p4 = payload_offset(&vols[3], chunk) as u64;
    let h3 = vec![(p3 + 10_000, 20_000), (p3 + 70_000, 5_000)];
    let h4 = vec![(p4, 4_000)];
    let mut dmg3 = vols[2].clone();
    for &(s, l) in &h3 {
        dmg3[s as usize..(s + l) as usize].fill(0);
    }
    let mut dmg4 = vols[3].clone();
    for &(s, l) in &h4 {
        dmg4[s as usize..(s + l) as usize].fill(0);
    }
    std::fs::write(&v3, &dmg3).unwrap();
    std::fs::write(&v4, &dmg4).unwrap();
    let feed = Damaged {
        holes: vec![(v3.clone(), h3), (v4.clone(), h4)],
        repair: vec![(v3, vols[2].clone()), (v4, vols[3].clone())],
        handed: false,
    };
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(
        log.iter().any(|l| l.contains("extracted past damage")),
        "{log:?}"
    );
}

#[test]
fn unrepaired_holes_fail_as_damage() {
    let t = TestDir::new("unrepaired");
    let data = pattern(300_000);
    let chunk = 100_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let p2 = payload_offset(&vols[1], chunk) as u64;
    struct NoRepair(PathBuf, u64);
    impl VolumeFeed for NoRepair {
        fn volume(&mut self, path: &Path) -> VolumeView {
            if path == self.0 {
                VolumeView::Ready {
                    holes: vec![(self.1, 1000)],
                }
            } else if path.is_file() {
                VolumeView::Ready { holes: Vec::new() }
            } else {
                VolumeView::Missing
            }
        }
    }
    let (r, log) = run(&first, &t.out(), Box::new(NoRepair(v2, p2 + 500)));
    match r {
        Err(Error::Failed(f)) => assert!(f.is_damage(), "{f:?}"),
        _ => panic!("expected damage failure: {log:?}"),
    }
    assert!(!t.out().join("movie.mkv").exists());
}

#[test]
fn rar4_solid_first_member_without_history_is_refused() {
    // A RAR4 (1.5-4.x) archive whose first member is solid and compressed (method 0x33).
    let t = TestDir::new("rar4c");
    let mut v = b"Rar!\x1a\x07\x00".to_vec();
    // main header
    let mut mh = vec![0x73, 0x00, 0x00, 13, 0, 0, 0, 0, 0, 0, 0];
    let crc = (crc32fast::hash(&mh) & 0xffff) as u16;
    let mut b = crc.to_le_bytes().to_vec();
    b.append(&mut mh);
    v.extend(b);
    let packed = vec![0u8; 64];
    let name = b"x.bin";
    let mut fh = vec![0x74u8];
    fh.extend_from_slice(&(0x8000u16 | 0x0010).to_le_bytes()); // LONG_BLOCK | SOLID
    fh.extend_from_slice(&((32 + name.len()) as u16).to_le_bytes());
    fh.extend_from_slice(&(packed.len() as u32).to_le_bytes());
    fh.extend_from_slice(&100u32.to_le_bytes());
    fh.push(3);
    fh.extend_from_slice(&0u32.to_le_bytes());
    fh.extend_from_slice(&0u32.to_le_bytes());
    fh.push(29);
    fh.push(0x33);
    fh.extend_from_slice(&(name.len() as u16).to_le_bytes());
    fh.extend_from_slice(&0x20u32.to_le_bytes());
    fh.extend_from_slice(name);
    let crc = (crc32fast::hash(&fh) & 0xffff) as u16;
    v.extend(crc.to_le_bytes());
    v.extend(fh);
    v.extend(&packed);
    let first = t.dl().join("old.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    // zipfast decodes RAR4 solid archives now; a solid first member has no history to
    // continue and its data is garbage, so it is refused (as damaged, no longer unsupported).
    assert!(r.is_err(), "log: {log:?}");
}

#[test]
fn orphan_continuation_volume_is_refused() {
    // Ported from archive_native: a set that starts with a split-before member.
    let t = TestDir::new("orphan");
    let payload = b"tail-bytes";
    let mut v = SIG.to_vec();
    v.extend(main_block(true, None));
    v.extend(file_block(
        "movie.bin",
        payload.len() as u64,
        payload,
        crc32fast::hash(payload),
        true,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("orphan.part2.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    assert!(
        r.is_err(),
        "an orphan continuation must not extract: {log:?}"
    );
    assert!(!t.out().join("movie.bin").exists());
}

// ---- RAR4 (RAR 2.9/3.x/4.x) stored fixtures with valid CRCs ----

/// A RAR4 header: CRC16 (low half of CRC32 over everything after the CRC field) + body.
fn rar4_block(kind: u8, flags: u16, rest: &[u8], add_size: usize) -> Vec<u8> {
    let mut body = vec![kind];
    body.extend_from_slice(&flags.to_le_bytes());
    body.extend_from_slice(&((7 + rest.len()) as u16).to_le_bytes());
    body.extend_from_slice(rest);
    let _ = add_size;
    let crc = (crc32fast::hash(&body) & 0xffff) as u16;
    let mut out = crc.to_le_bytes().to_vec();
    out.extend(body);
    out
}

const DOS_TIME: u32 = (46 << 25) | (1 << 21) | (1 << 16); // 2026-01-01 00:00

fn rar4_file_header(
    name: &str,
    packed: usize,
    size: usize,
    crc: u32,
    before: bool,
    after: bool,
    salt: Option<&[u8; 8]>,
) -> Vec<u8> {
    let mut flags: u16 = 0x8000; // LONG_BLOCK
    if before {
        flags |= 0x0001;
    }
    if after {
        flags |= 0x0002;
    }
    if salt.is_some() {
        flags |= 0x0004 | 0x0400;
    }
    let mut rest = Vec::new();
    rest.extend_from_slice(&(packed as u32).to_le_bytes());
    rest.extend_from_slice(&(size as u32).to_le_bytes());
    rest.push(3); // host OS: Unix
    rest.extend_from_slice(&crc.to_le_bytes());
    rest.extend_from_slice(&DOS_TIME.to_le_bytes());
    rest.push(29); // unpack version
    rest.push(0x30); // stored
    rest.extend_from_slice(&(name.len() as u16).to_le_bytes());
    rest.extend_from_slice(&0o100644u32.to_le_bytes());
    rest.extend_from_slice(name.as_bytes());
    if let Some(s) = salt {
        rest.extend_from_slice(s);
    }
    rar4_block(0x74, flags, &rest, packed)
}

fn rar4_volume(header: Vec<u8>, payload: &[u8], more: bool, first: bool) -> Vec<u8> {
    let mut v = b"Rar!\x1a\x07\x00".to_vec();
    let mut mflags: u16 = 0x0001; // volume
    if first {
        mflags |= 0x0100; // first volume
    }
    mflags |= 0x0010; // new numbering
    v.extend(rar4_block(0x73, mflags, &[0u8; 6], 0));
    v.extend(header);
    v.extend_from_slice(payload);
    let eflags: u16 = if more { 0x0001 } else { 0 };
    v.extend(rar4_block(0x7b, eflags, &[], 0));
    v
}

pub(crate) fn rar4_store_volumes(name: &str, data: &[u8], chunk: usize) -> Vec<Vec<u8>> {
    let parts: Vec<&[u8]> = data.chunks(chunk).collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let last = i + 1 == n;
            let crc = if last {
                crc32fast::hash(data)
            } else {
                crc32fast::hash(part)
            };
            let h = rar4_file_header(name, part.len(), data.len(), crc, i > 0, !last, None);
            rar4_volume(h, part, !last, i == 0)
        })
        .collect()
}

const RAR4_SALT: [u8; 8] = [0x92, 0x80, 0x37, 0x09, 0x6e, 0x88, 0x92, 0xdb];

/// Salted AES-128 stored member, one CBC stream across all volumes, cut at `chunk` bytes.
fn rar4_aes_store_volumes(name: &str, data: &[u8], chunk: usize, password: &str) -> Vec<Vec<u8>> {
    let (key, iv) = zipfast::crypto::derive_rar4_key_iv(password, &RAR4_SALT);
    let mut padded = data.to_vec();
    padded.resize(data.len().next_multiple_of(16), 0);
    let cipher = zipfast::crypto::encrypt_cbc(&key, &iv, &padded).unwrap();
    let starts: Vec<usize> = (0..data.len()).step_by(chunk).collect();
    let n = starts.len();
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let last = i + 1 == n;
            let end = if last { cipher.len() } else { start + chunk };
            let part = &cipher[start..end];
            // Non-final parts carry the CRC of their packed (cipher) bytes, the last one the
            // whole plaintext CRC.
            let crc = if last {
                crc32fast::hash(data)
            } else {
                crc32fast::hash(part)
            };
            let h = rar4_file_header(
                name,
                part.len(),
                data.len(),
                crc,
                i > 0,
                !last,
                Some(&RAR4_SALT),
            );
            rar4_volume(h, part, !last, i == 0)
        })
        .collect()
}

fn run_pw(
    first: &Path,
    out: &Path,
    password: &str,
    feed: Box<dyn VolumeFeed>,
) -> (Result<zipfast::extract::Summary, Error>, Vec<String>) {
    let mut log = Vec::new();
    let req = Request {
        first_volume: first,
        dest: out,
        password,
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let r = extract(
        &req,
        feed,
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    (r, log)
}

fn rar4_name(i: usize) -> String {
    format!("old.part{:02}.rar", i + 1)
}

fn write_named(dir: &Path, vols: &[Vec<u8>], name: fn(usize) -> String) -> PathBuf {
    for (i, v) in vols.iter().enumerate() {
        std::fs::write(dir.join(name(i)), v).unwrap();
    }
    dir.join(name(0))
}

#[test]
fn rar4_store_multi_volume_extracts() {
    let t = TestDir::new("rar4s");
    let data = pattern(250_000);
    let first = write_named(
        &t.dl(),
        &rar4_store_volumes("movie.mkv", &data, 60_000),
        rar4_name,
    );
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn rar4_aes_store_multi_volume_extracts() {
    let t = TestDir::new("rar4aes");
    let data = pattern(200_003);
    let vols = rar4_aes_store_volumes("movie.mkv", &data, 50_001, "secret");
    let first = write_named(&t.dl(), &vols, rar4_name);
    let (r, log) = run_pw(&first, &t.out(), "secret", Box::new(CompleteSet));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn rar4_aes_wrong_password_is_typed() {
    let t = TestDir::new("rar4aeswrong");
    let data = pattern(100_000);
    let vols = rar4_aes_store_volumes("movie.mkv", &data, 40_000, "secret");
    let first = write_named(&t.dl(), &vols, rar4_name);
    let (r, log) = run_pw(&first, &t.out(), "nope", Box::new(CompleteSet));
    match r {
        Err(Error::Failed(f)) => assert!(
            f.is_password() || matches!(f, Failure::EncryptedChecksum(_)),
            "RAR4 has no password verifier: wrong password shows as a checksum failure or password failure, got {f:?}"
        ),
        _ => panic!("wrong password must fail: {log:?}"),
    }
    assert!(!t.out().join("movie.mkv").exists());
    // The unpacker's password list moves on to the next candidate on a decryption error.
    assert!(
        log.iter().any(|l| {
            let info = crate::unpack::parse_unpack_message(l.as_bytes(), b"", false);
            info.decrypt_error != 0 || info.password_error != 0
        }),
        "{log:?}"
    );
}

fn rar4_payload_offset(vol: &[u8], part_len: usize) -> usize {
    vol.len() - 7 - part_len
}

#[test]
fn rar4_store_holes_patched() {
    let t = TestDir::new("rar4holes");
    let data = pattern(400_000);
    let chunk = 100_000;
    let vols = rar4_store_volumes("movie.mkv", &data, chunk);
    let first = write_named(&t.dl(), &vols, rar4_name);
    let v2 = t.dl().join(rar4_name(1));
    let p2 = rar4_payload_offset(&vols[1], chunk) as u64;
    let h2 = vec![(p2 + 30_000, 7_000)];
    let mut dmg = vols[1].clone();
    dmg[(p2 + 30_000) as usize..(p2 + 37_000) as usize].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(v2.clone(), h2)],
        repair: vec![(v2, vols[1].clone())],
        handed: false,
    };
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn rar4_aes_store_holes_patched() {
    let t = TestDir::new("rar4aesholes");
    let data = pattern(300_000);
    let chunk = 75_001;
    let vols = rar4_aes_store_volumes("movie.mkv", &data, chunk, "secret");
    let first = write_named(&t.dl(), &vols, rar4_name);
    let v2 = t.dl().join(rar4_name(1));
    let p2 = rar4_payload_offset(&vols[1], chunk) as u64;
    let h2 = vec![(p2 + 20_005, 3_000)];
    let mut dmg = vols[1].clone();
    dmg[(p2 + 20_005) as usize..(p2 + 23_005) as usize].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(v2.clone(), h2)],
        repair: vec![(v2, vols[1].clone())],
        handed: false,
    };
    let (r, log) = run_pw(&first, &t.out(), "secret", Box::new(feed));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

// ---- RAR5 with encrypted headers and data (-hp) ----

const RAR5_SALT: [u8; 16] = [3; 16];
const RAR5_IV: [u8; 16] = [9; 16];

fn rar5_enc_file_header(
    name: &str,
    packed: u64,
    size: u64,
    crc: u32,
    before: bool,
    after: bool,
) -> Vec<u8> {
    let mut fields = Vec::new();
    vint(0x4, &mut fields); // CRC present
    vint(size, &mut fields);
    vint(0x20, &mut fields);
    fields.extend(crc.to_le_bytes());
    vint(0, &mut fields); // stored
    vint(0, &mut fields); // Windows host
    vint(name.len() as u64, &mut fields);
    fields.extend(name.as_bytes());
    let mut extra = vec![36, 1, 0, 0, 1]; // size, crypt record, version 0, flags 0, KDF exponent 1
    extra.extend(RAR5_SALT);
    extra.extend(RAR5_IV);
    let mut body = Vec::new();
    vint(2, &mut body);
    vint(
        3 | if before { 8 } else { 0 } | if after { 16 } else { 0 },
        &mut body,
    );
    vint(extra.len() as u64, &mut body);
    vint(packed, &mut body);
    body.extend(fields);
    body.extend(extra);
    let mut sized = Vec::new();
    vint(body.len() as u64, &mut sized);
    sized.extend(body);
    let mut out = crc32fast::hash(&sized).to_le_bytes().to_vec();
    out.extend(sized);
    out
}

fn rar5_wire(mut h: Vec<u8>, key: &[u8; 32], ordinal: u8) -> Vec<u8> {
    h.resize(h.len().next_multiple_of(16), 0);
    let iv = [ordinal + 30; 16];
    let mut out = iv.to_vec();
    out.extend(zipfast::crypto::encrypt_cbc(key, &iv, &h).unwrap());
    out
}

/// Encrypted headers + encrypted stored data, one CBC stream over all volumes. Returns the
/// volumes and each volume's payload offset.
fn rar5_hp_volumes(
    name: &str,
    data: &[u8],
    chunk: usize,
    password: &str,
) -> (Vec<Vec<u8>>, Vec<usize>) {
    let key = zipfast::crypto::derive_rar5_key_and_hash_key(password, 1, &RAR5_SALT, None)
        .unwrap()
        .0;
    let mut padded = data.to_vec();
    padded.resize(data.len().next_multiple_of(16), 0);
    let cipher = zipfast::crypto::encrypt_cbc(&key, &RAR5_IV, &padded).unwrap();
    let parts: Vec<&[u8]> = cipher.chunks(chunk).collect();
    let n = parts.len();
    let mut vols = Vec::new();
    let mut offsets = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == n;
        let mut v = SIG.to_vec();
        let mut params = vec![0, 0, 1];
        params.extend(RAR5_SALT);
        v.extend(block(4, 0, &params, &[], false));
        v.extend(rar5_wire(
            main_block(true, (i > 0).then_some(i as u64)),
            &key,
            0,
        ));
        // Non-final parts carry the CRC of their packed (cipher) bytes, the last one the whole
        // plaintext CRC.
        let crc = if last {
            crc32fast::hash(data)
        } else {
            crc32fast::hash(part)
        };
        v.extend(rar5_wire(
            rar5_enc_file_header(
                name,
                part.len() as u64,
                data.len() as u64,
                crc,
                i > 0,
                !last,
            ),
            &key,
            1,
        ));
        offsets.push(v.len());
        v.extend_from_slice(part);
        v.extend(rar5_wire(end_block(!last), &key, 2));
        vols.push(v);
    }
    (vols, offsets)
}

#[test]
fn rar5_encrypted_headers_extract() {
    let t = TestDir::new("rar5hp");
    let data = pattern(250_000);
    let (vols, _) = rar5_hp_volumes("movie.mkv", &data, 64_000, "pw");
    let first = write_set(&t.dl(), &vols);
    let (r, log) = run_pw(&first, &t.out(), "pw", Box::new(CompleteSet));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn rar5_encrypted_headers_missing_password_is_typed() {
    let t = TestDir::new("rar5hpnopw");
    let data = pattern(100_000);
    let (vols, _) = rar5_hp_volumes("movie.mkv", &data, 64_000, "pw");
    let first = write_set(&t.dl(), &vols);
    let (r, log) = run_pw(&first, &t.out(), "", Box::new(CompleteSet));
    match r {
        Err(Error::Failed(f)) => assert!(f.is_password(), "{f:?} {log:?}"),
        _ => panic!("expected a password failure: {log:?}"),
    }
}

#[test]
fn rar5_encrypted_store_holes_patched() {
    let t = TestDir::new("rar5hpholes");
    let data = pattern(320_000);
    let (vols, offs) = rar5_hp_volumes("movie.mkv", &data, 80_000, "pw");
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let p2 = offs[1] as u64;
    let h2 = vec![(p2 + 10_007, 2_000)];
    let mut dmg = vols[1].clone();
    dmg[(p2 + 10_007) as usize..(p2 + 12_007) as usize].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(v2.clone(), h2)],
        repair: vec![(v2, vols[1].clone())],
        handed: false,
    };
    let (r, log) = run_pw(&first, &t.out(), "pw", Box::new(feed));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn hole_longer_than_a_read_is_answered_in_pieces() {
    // Two lost 1 MiB articles: longer than zipfast's 1 MiB read chunk.
    let t = TestDir::new("bighole");
    let data = pattern(6_000_000);
    let chunk = 3_000_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v1 = t.dl().join(volume_name(0, vols.len()));
    let p1 = payload_offset(&vols[0], chunk) as u64;
    let h1 = vec![(p1 + 500_000, 2 * 1_048_576)];
    let mut dmg = vols[0].clone();
    dmg[(p1 + 500_000) as usize..(p1 + 500_000 + 2 * 1_048_576) as usize].fill(0);
    std::fs::write(&v1, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(v1.clone(), h1)],
        repair: vec![(v1, vols[0].clone())],
        handed: false,
    };
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

/// A download feed (not a finished set): outputs are copied into place.
struct Growing;
impl VolumeFeed for Growing {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else {
            VolumeView::Missing
        }
    }
}

#[test]
fn many_members_while_downloading_do_not_exhaust_artifact_slots() {
    let t = TestDir::new("many");
    let files: Vec<(String, Vec<u8>)> = (0..40)
        .map(|i| (format!("dir/f{i:02}.bin"), pattern(1_000 + i * 37)))
        .collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    for (name, data) in &files {
        v.extend(file_block(
            name,
            data.len() as u64,
            data,
            crc32fast::hash(data),
            false,
            false,
        ));
    }
    v.extend(end_block(false));
    let first = t.dl().join("many.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(Growing));
    let s = expect_ok(r, &log);
    assert_eq!(s.files, 40);
    for (name, data) in &files {
        assert_eq!(&std::fs::read(t.out().join(name)).unwrap(), data, "{name}");
    }
}

fn file_block_mode(name: &str, data: &[u8], mode: u64) -> Vec<u8> {
    let mut fields = Vec::new();
    vint(0x4 | 0x2, &mut fields);
    vint(data.len() as u64, &mut fields);
    vint(mode, &mut fields);
    fields.extend_from_slice(&1_700_000_000u32.to_le_bytes());
    fields.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
    vint(0, &mut fields);
    vint(1, &mut fields); // unix host
    vint(name.len() as u64, &mut fields);
    fields.extend_from_slice(name.as_bytes());
    block(2, 0, &fields, data, true)
}

#[cfg(unix)]
#[test]
fn copied_outputs_keep_mode_and_mtime() {
    use std::os::unix::fs::PermissionsExt;
    let t = TestDir::new("meta");
    let data = pattern(5_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block_mode("tool.sh", &data, 0o755));
    v.extend(end_block(false));
    let first = t.dl().join("meta.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(Growing));
    expect_ok(r, &log);
    let meta = std::fs::metadata(t.out().join("tool.sh")).unwrap();
    assert_eq!(
        meta.permissions().mode() & 0o111,
        0o111,
        "executable bits restored"
    );
    let mtime = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(mtime, 1_700_000_000);
}

/// A `.rustyget-<pid>-<n>.part` a crashed daemon left goes with the next copy into that
/// folder; this process's own, a running process's and other names stay.
#[cfg(unix)]
#[test]
fn a_crashed_daemons_part_files_go_with_the_next_copy() {
    use std::os::unix::io::AsRawFd;
    let t = TestDir::new("staleparts");
    // Above pid_max on Linux (at most 2^22) and macOS (99998): never a running process.
    let dead = 99_999_999u32;
    let stale = t.out().join(format!(".rustyget-{dead}-7.part"));
    let own = t.out().join(format!(".rustyget-{}-99999.part", std::process::id()));
    let running = t.out().join(".rustyget-1-3.part"); // init/launchd: always running
    let other = t.out().join(format!(".rustyget-{dead}-x.part"));
    let stale_dir = t.out().join(format!(".rustyget-{dead}-8.part"));
    // A writer elsewhere (another host or PID namespace) holding its part file's lock.
    let locked = t.out().join(format!(".rustyget-{dead}-9.part"));
    for f in [&stale, &own, &running, &other, &locked] {
        std::fs::write(f, b"partial").unwrap();
    }
    std::fs::create_dir(&stale_dir).unwrap();
    let holder = std::fs::OpenOptions::new().write(true).open(&locked).unwrap();
    assert_eq!(unsafe { libc::flock(holder.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    let data = pattern(50_000);
    let first = write_set(&t.dl(), &rar5_volumes("movie.mkv", &data, 1 << 30));
    let (r, log) = run(&first, &t.out(), Box::new(Growing));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(!stale.exists(), "the crashed daemon's part file is removed");
    assert!(own.exists() && running.exists() && other.exists());
    assert!(locked.exists(), "a part file whose lock is held is in use");
    assert!(stale_dir.is_dir(), "never a directory");
    drop(holder);
    assert_eq!(remove_stale_parts(&t.out()), 1, "unlocked, it goes");
    assert!(!locked.exists());
    // The writer's own part file is locked while it is written.
    let (part, file) = create_unique_sibling(&t.out().join("x.mkv")).unwrap();
    assert!(try_lock_unused(&part).is_none());
    drop(file);
    assert!(try_lock_unused(&part).is_some());
    std::fs::remove_file(part).unwrap();
    assert_eq!(stale_part_pid(".rustyget-123-4.part"), Some(123));
    for name in [".rustyget-0-4.part", ".rustyget-12-.part", ".rustyget--4.part", ".rustyget-1-2.tmp"] {
        assert_eq!(stale_part_pid(name), None, "{name}");
    }
}

#[test]
fn copying_into_place_never_touches_other_paths() {
    let t = TestDir::new("tmpname");
    // Files that a predictable temporary name could collide with.
    std::fs::write(t.out().join("movie.mkv.rgpart"), b"keep me").unwrap();
    let data = pattern(50_000);
    let first = write_set(&t.dl(), &rar5_volumes("movie.mkv", &data, 1 << 30));
    let (r, log) = run(&first, &t.out(), Box::new(Growing));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert_eq!(
        std::fs::read(t.out().join("movie.mkv.rgpart")).unwrap(),
        b"keep me"
    );
    let leftovers: Vec<_> = std::fs::read_dir(t.out())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".rustyget-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn rar4_stored_then_broken_solid_member_fails() {
    let t = TestDir::new("rar4mixed");
    let stored = pattern(20_000);
    let mut v = b"Rar!\x1a\x07\x00".to_vec();
    v.extend(rar4_block(0x73, 0, &[0u8; 6], 0));
    v.extend(rar4_file_header(
        "a.bin",
        stored.len(),
        stored.len(),
        crc32fast::hash(&stored),
        false,
        false,
        None,
    ));
    v.extend_from_slice(&stored);
    // A compressed member (method 0x33) after it.
    let packed = vec![0u8; 64];
    let mut rest = Vec::new();
    rest.extend_from_slice(&(packed.len() as u32).to_le_bytes());
    rest.extend_from_slice(&100u32.to_le_bytes());
    rest.push(3);
    rest.extend_from_slice(&0u32.to_le_bytes());
    rest.extend_from_slice(&DOS_TIME.to_le_bytes());
    rest.push(29);
    rest.push(0x33);
    rest.extend_from_slice(&5u16.to_le_bytes());
    rest.extend_from_slice(&0o100644u32.to_le_bytes());
    rest.extend_from_slice(b"b.bin");
    // Solid compressed with garbage data: zipfast decodes RAR4 solid now and fails on it.
    v.extend(rar4_block(0x74, 0x8000 | 0x0010, &rest, packed.len()));
    v.extend(&packed);
    v.extend(rar4_block(0x7b, 0, &[], 0));
    let first = t.dl().join("mixed.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    assert!(r.is_err(), "log: {log:?}");
}

#[test]
fn holes_in_a_member_inside_a_folder_are_patched() {
    let t = TestDir::new("holesdir");
    let data = pattern(300_000);
    let chunk = 100_000;
    let vols = rar5_volumes("Release/movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let p2 = payload_offset(&vols[1], chunk) as u64;
    let mut dmg = vols[1].clone();
    dmg[(p2 + 40_000) as usize..(p2 + 45_000) as usize].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(v2.clone(), vec![(p2 + 40_000, 5_000)])],
        repair: vec![(v2, vols[1].clone())],
        handed: false,
    };
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    expect_ok(r, &log);
    assert_eq!(
        std::fs::read(t.out().join("Release/movie.mkv")).unwrap(),
        data
    );
}

#[test]
fn rename_mode_keeps_duplicate_members_and_existing_files() {
    let t = TestDir::new("rename");
    std::fs::write(t.out().join("a.bin"), b"existing").unwrap();
    let one = pattern(3_000);
    let two = pattern(4_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block(
        "a.bin",
        one.len() as u64,
        &one,
        crc32fast::hash(&one),
        false,
        false,
    ));
    v.extend(file_block(
        "a.bin",
        two.len() as u64,
        &two,
        crc32fast::hash(&two),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("dup.rar");
    std::fs::write(&first, &v).unwrap();
    let mut log = Vec::new();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Rename,
        on_wait: None,
    };
    let r = extract(
        &req,
        Box::new(CompleteSet),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("a.bin")).unwrap(), b"existing");
    assert_eq!(std::fs::read(t.out().join("a_1.bin")).unwrap(), one);
    assert_eq!(std::fs::read(t.out().join("a_2.bin")).unwrap(), two);
}

#[test]
fn holes_in_a_secondary_member_are_patched_by_re_extracting() {
    let t = TestDir::new("secondary");
    let big = pattern(300_000);
    let extra: Vec<u8> = pattern(150_000).iter().map(|b| b ^ 0x5a).collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block(
        "big.mkv",
        big.len() as u64,
        &big,
        crc32fast::hash(&big),
        false,
        false,
    ));
    let extra_at =
        v.len() + file_block("extra.bin", extra.len() as u64, &[], 0, false, false).len();
    v.extend(file_block(
        "extra.bin",
        extra.len() as u64,
        &extra,
        crc32fast::hash(&extra),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("two.rar");
    let mut dmg = v.clone();
    let hole = (extra_at as u64 + 70_000, 4_000u64);
    dmg[hole.0 as usize..(hole.0 + hole.1) as usize].fill(0);
    std::fs::write(&first, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(first.clone(), vec![hole])],
        repair: vec![(first.clone(), v.clone())],
        handed: false,
    };
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("big.mkv")).unwrap(), big);
    assert_eq!(std::fs::read(t.out().join("extra.bin")).unwrap(), extra);
    let leftovers: Vec<_> = std::fs::read_dir(t.out())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".rustyget-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

fn file_block_no_crc(name: &str, data: &[u8]) -> Vec<u8> {
    let mut fields = Vec::new();
    vint(0x2, &mut fields); // mtime only, no CRC
    vint(data.len() as u64, &mut fields);
    vint(0o644, &mut fields);
    fields.extend_from_slice(&1_700_000_000u32.to_le_bytes());
    vint(0, &mut fields);
    vint(1, &mut fields);
    vint(name.len() as u64, &mut fields);
    fields.extend_from_slice(name.as_bytes());
    block(2, 0, &fields, data, true)
}

#[test]
fn intact_member_without_checksum_is_extracted_while_downloading() {
    let t = TestDir::new("nocrc");
    let data = pattern(40_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block_no_crc("plain.bin", &data));
    v.extend(end_block(false));
    let first = t.dl().join("nocrc.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(Growing));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("plain.bin")).unwrap(), data);
}

#[test]
fn a_repaired_member_does_not_replace_a_later_member_with_the_same_name() {
    let t = TestDir::new("lastwins");
    let first_copy = pattern(200_000);
    let second_copy: Vec<u8> = pattern(50_000).iter().map(|b| b ^ 0x33).collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    let at = v.len() + file_block("a.bin", first_copy.len() as u64, &[], 0, false, false).len();
    v.extend(file_block(
        "a.bin",
        first_copy.len() as u64,
        &first_copy,
        crc32fast::hash(&first_copy),
        false,
        false,
    ));
    v.extend(file_block(
        "a.bin",
        second_copy.len() as u64,
        &second_copy,
        crc32fast::hash(&second_copy),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("dups.rar");
    let hole = (at as u64 + 100_000, 3_000u64);
    let mut dmg = v.clone();
    dmg[hole.0 as usize..(hole.0 + hole.1) as usize].fill(0);
    std::fs::write(&first, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(first.clone(), vec![hole])],
        repair: vec![(first.clone(), v.clone())],
        handed: false,
    };
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("a.bin")).unwrap(), second_copy);
}

#[cfg(unix)]
#[test]
fn duplicate_names_keep_their_own_metadata() {
    use std::os::unix::fs::PermissionsExt;
    let t = TestDir::new("dupmeta");
    let one = pattern(2_000);
    let two = pattern(3_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block_mode("script.sh", &one, 0o644));
    v.extend(file_block_mode("script.sh", &two, 0o755));
    v.extend(end_block(false));
    let first = t.dl().join("dupmeta.rar");
    std::fs::write(&first, &v).unwrap();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Rename,
        on_wait: None,
    };
    let r = extract(&req, Box::new(Growing), &mut |_, _| {}, &mut || false);
    expect_ok(r, &[]);
    let mode = |n: &str| {
        std::fs::metadata(t.out().join(n))
            .unwrap()
            .permissions()
            .mode()
    };
    assert_eq!(mode("script.sh") & 0o111, 0, "first copy keeps 0644");
    assert_eq!(mode("script_1.sh") & 0o111, 0o111, "second copy keeps 0755");
}

#[test]
fn skip_mode_keeps_the_first_of_duplicate_names() {
    // The first member has no checksum (held back), the second has one (placed at once):
    // skipping existing files must still keep the first, as sequential extraction does.
    let t = TestDir::new("firstwins");
    let one = pattern(5_000);
    let two: Vec<u8> = pattern(6_000).iter().map(|b| b ^ 0x11).collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block_no_crc("a.bin", &one));
    v.extend(file_block(
        "a.bin",
        two.len() as u64,
        &two,
        crc32fast::hash(&two),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("first.rar");
    std::fs::write(&first, &v).unwrap();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Skip,
        on_wait: None,
    };
    let r = extract(&req, Box::new(Growing), &mut |_, _| {}, &mut || false);
    expect_ok(r, &[]);
    assert_eq!(std::fs::read(t.out().join("a.bin")).unwrap(), one);
}

#[test]
fn repairing_a_duplicate_name_uses_its_own_bytes() {
    // Rename mode, a damaged first "doc.bin" (not the set's main payload, so the re-extraction
    // fallback is used) followed by another "doc.bin" with other bytes.
    let t = TestDir::new("dupfix");
    let main = pattern(400_000);
    let one = pattern(120_000);
    let two: Vec<u8> = pattern(120_000).iter().map(|b| b ^ 0x77).collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block(
        "main.mkv",
        main.len() as u64,
        &main,
        crc32fast::hash(&main),
        false,
        false,
    ));
    let at = v.len() + file_block("doc.bin", one.len() as u64, &[], 0, false, false).len();
    v.extend(file_block(
        "doc.bin",
        one.len() as u64,
        &one,
        crc32fast::hash(&one),
        false,
        false,
    ));
    v.extend(file_block(
        "doc.bin",
        two.len() as u64,
        &two,
        crc32fast::hash(&two),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("dupfix.rar");
    let hole = (at as u64 + 50_000, 2_000u64);
    let mut dmg = v.clone();
    dmg[hole.0 as usize..(hole.0 + hole.1) as usize].fill(0);
    std::fs::write(&first, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(first.clone(), vec![hole])],
        repair: vec![(first.clone(), v.clone())],
        handed: false,
    };
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Rename,
        on_wait: None,
    };
    let mut log = Vec::new();
    let r = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r, &log);
    let mut got = vec![
        std::fs::read(t.out().join("doc.bin")).unwrap(),
        std::fs::read(t.out().join("doc_1.bin")).unwrap(),
    ];
    got.sort();
    let mut want = vec![one, two];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn repair_reads_the_right_copy_when_a_member_is_named_like_a_renamed_one() {
    // doc.bin, doc_1.bin, then a damaged second doc.bin (re-extraction names it doc_2.bin).
    let t = TestDir::new("renamedlike");
    let main = pattern(400_000);
    let a: Vec<u8> = pattern(60_000).iter().map(|b| b ^ 0x01).collect();
    let b: Vec<u8> = pattern(60_000).iter().map(|b| b ^ 0x02).collect();
    let c: Vec<u8> = pattern(90_000).iter().map(|b| b ^ 0x03).collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block(
        "main.mkv",
        main.len() as u64,
        &main,
        crc32fast::hash(&main),
        false,
        false,
    ));
    v.extend(file_block(
        "doc.bin",
        a.len() as u64,
        &a,
        crc32fast::hash(&a),
        false,
        false,
    ));
    v.extend(file_block(
        "doc_1.bin",
        b.len() as u64,
        &b,
        crc32fast::hash(&b),
        false,
        false,
    ));
    let at = v.len() + file_block("doc.bin", c.len() as u64, &[], 0, false, false).len();
    v.extend(file_block(
        "doc.bin",
        c.len() as u64,
        &c,
        crc32fast::hash(&c),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("rl.rar");
    let hole = (at as u64 + 30_000, 2_000u64);
    let mut dmg = v.clone();
    dmg[hole.0 as usize..(hole.0 + hole.1) as usize].fill(0);
    std::fs::write(&first, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(first.clone(), vec![hole])],
        repair: vec![(first.clone(), v.clone())],
        handed: false,
    };
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Rename,
        on_wait: None,
    };
    let mut log = Vec::new();
    let r = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r, &log);
    let mut got: Vec<Vec<u8>> = ["doc.bin", "doc_1.bin", "doc_2.bin"]
        .iter()
        .map(|n| std::fs::read(t.out().join(n)).unwrap())
        .collect();
    got.sort();
    let mut want = vec![a, b, c];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn waiting_for_a_volume_is_reported() {
    let t = TestDir::new("onwait");
    let data = pattern(300_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let names: Vec<PathBuf> = (0..vols.len())
        .map(|i| t.dl().join(volume_name(i, vols.len())))
        .collect();
    let released = Arc::new(AtomicUsize::new(1));
    let feed = Trickle {
        released: released.clone(),
        names,
        asked: Arc::new(AtomicUsize::new(0)),
    };
    let rel = released.clone();
    let n = vols.len();
    let releaser = std::thread::spawn(move || {
        for k in 2..=n {
            std::thread::sleep(Duration::from_millis(120));
            rel.store(k, Ordering::Release);
        }
    });
    let seen = Mutex::new(Vec::new());
    let on_wait = |w: bool| seen.lock().unwrap().push(w);
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: Some(&on_wait),
    };
    let r = extract(&req, Box::new(feed), &mut |_, _| {}, &mut || false);
    releaser.join().unwrap();
    expect_ok(r, &[]);
    let seen = seen.into_inner().unwrap();
    assert!(seen.contains(&true), "{seen:?}");
    assert_eq!(seen.last(), Some(&false), "{seen:?}");
}

#[test]
fn rename_mode_out_of_slots_behind_damage_reports_damage() {
    let t = TestDir::new("slots");
    let first_copy = pattern(80_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    let at = v.len() + file_block("doc.bin", first_copy.len() as u64, &[], 0, false, false).len();
    v.extend(file_block(
        "doc.bin",
        first_copy.len() as u64,
        &first_copy,
        crc32fast::hash(&first_copy),
        false,
        false,
    ));
    for i in 0..24u8 {
        let d: Vec<u8> = pattern(1_000).iter().map(|b| b ^ i).collect();
        v.extend(file_block(
            "doc.bin",
            d.len() as u64,
            &d,
            crc32fast::hash(&d),
            false,
            false,
        ));
    }
    v.extend(end_block(false));
    let first = t.dl().join("slots.rar");
    let hole = (at as u64 + 40_000, 1_000u64);
    let mut dmg = v.clone();
    dmg[hole.0 as usize..(hole.0 + hole.1) as usize].fill(0);
    std::fs::write(&first, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(first.clone(), vec![hole])],
        repair: vec![(first.clone(), v.clone())],
        handed: false,
    };
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Rename,
        on_wait: None,
    };
    let r = extract(&req, Box::new(feed), &mut |_, _| {}, &mut || false);
    match r {
        Err(Error::Failed(f)) => assert!(f.is_damage(), "{f:?}"),
        Ok(_) => panic!("must not succeed with renamed copies out of archive order"),
        Err(Error::Unsupported(u)) => panic!("{u:?}"),
    }
}

#[test]
fn rename_mode_names_follow_archive_order() {
    // a.bin (no checksum), a.bin, a_1.bin: sequential extraction gives a.bin, a_1.bin, a_1_1.bin.
    let t = TestDir::new("renameorder");
    let one = pattern(1_000);
    let two: Vec<u8> = pattern(1_100).iter().map(|b| b ^ 1).collect();
    let three: Vec<u8> = pattern(1_200).iter().map(|b| b ^ 2).collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block_no_crc("a.bin", &one));
    v.extend(file_block(
        "a.bin",
        two.len() as u64,
        &two,
        crc32fast::hash(&two),
        false,
        false,
    ));
    v.extend(file_block(
        "a_1.bin",
        three.len() as u64,
        &three,
        crc32fast::hash(&three),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("order.rar");
    std::fs::write(&first, &v).unwrap();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Rename,
        on_wait: None,
    };
    let r = extract(&req, Box::new(Growing), &mut |_, _| {}, &mut || false);
    expect_ok(r, &[]);
    assert_eq!(std::fs::read(t.out().join("a.bin")).unwrap(), one);
    assert_eq!(std::fs::read(t.out().join("a_1.bin")).unwrap(), two);
    assert_eq!(std::fs::read(t.out().join("a_1_1.bin")).unwrap(), three);
}

#[cfg(unix)]
#[test]
fn checksum_less_member_of_a_complete_set_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt;
    let t = TestDir::new("nocrcmode");
    let data = pattern(3_000);
    let mut fields = Vec::new();
    vint(0x2, &mut fields);
    vint(data.len() as u64, &mut fields);
    vint(0o755, &mut fields);
    fields.extend_from_slice(&1_700_000_000u32.to_le_bytes());
    vint(0, &mut fields);
    vint(1, &mut fields);
    vint(7, &mut fields);
    fields.extend_from_slice(b"tool.sh");
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(block(2, 0, &fields, &data, true));
    v.extend(end_block(false));
    let first = t.dl().join("nocrcmode.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    let meta = std::fs::metadata(t.out().join("tool.sh")).unwrap();
    assert_eq!(meta.permissions().mode() & 0o111, 0o111);
}

// ---- RAR4 LZ (methods 0x31-0x35), decoded by the session since zipfast #103 ----

fn lz_gold() -> Vec<u8> {
    let mut gold = b"RAR4 bounded LZ control.\n".repeat(8000);
    gold.extend((0u8..=255).cycle().take(256 * 400));
    gold
}

const LZ: [&[u8]; 5] = [
    include_bytes!("fixtures/official-lz-m1.packed"),
    include_bytes!("fixtures/official-lz-m2.packed"),
    include_bytes!("fixtures/official-lz-m3.packed"),
    include_bytes!("fixtures/official-lz-m4.packed"),
    include_bytes!("fixtures/official-lz-m5.packed"),
];

fn rar4_lz_member(name: &str, packed: &[u8], gold: &[u8], method: u8) -> Vec<u8> {
    let mut rest = Vec::new();
    rest.extend_from_slice(&(packed.len() as u32).to_le_bytes());
    rest.extend_from_slice(&(gold.len() as u32).to_le_bytes());
    rest.push(3);
    rest.extend_from_slice(&crc32fast::hash(gold).to_le_bytes());
    rest.extend_from_slice(&DOS_TIME.to_le_bytes());
    rest.push(29);
    rest.push(method);
    rest.extend_from_slice(&(name.len() as u16).to_le_bytes());
    rest.extend_from_slice(&0o100644u32.to_le_bytes());
    rest.extend_from_slice(name.as_bytes());
    let mut out = rar4_block(0x74, 0x8000, &rest, packed.len());
    out.extend_from_slice(packed);
    out
}

#[test]
fn rar4_lz_members_are_decoded_by_the_session() {
    let t = TestDir::new("rar4lz");
    let gold = lz_gold();
    let mut v = b"Rar!\x1a\x07\x00".to_vec();
    v.extend(rar4_block(0x73, 0, &[0u8; 6], 0));
    for (i, packed) in LZ.iter().enumerate() {
        v.extend(rar4_lz_member(
            &format!("m{}.bin", i + 1),
            packed,
            &gold,
            0x31 + i as u8,
        ));
    }
    v.extend(rar4_block(0x7b, 0x4000, &[], 0));
    let first = t.dl().join("lz.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    let s = expect_ok(r, &log);
    assert_eq!(s.files, 5);
    for i in 1..=5 {
        assert_eq!(
            std::fs::read(t.out().join(format!("m{i}.bin"))).unwrap(),
            gold,
            "m{i}"
        );
    }
}

#[test]
fn rar4_lz_members_across_volumes_while_downloading() {
    // One independent compressed member per volume, volumes arriving one at a time.
    let t = TestDir::new("rar4lzdl");
    let gold = lz_gold();
    let n = 3;
    for i in 0..n {
        let mut v = b"Rar!\x1a\x07\x00".to_vec();
        v.extend(rar4_block(
            0x73,
            0x0001 | 0x0010 | if i == 0 { 0x0100 } else { 0 },
            &[0u8; 6],
            0,
        ));
        v.extend(rar4_lz_member(&format!("part{i}.bin"), LZ[2], &gold, 0x33));
        v.extend(rar4_block(
            0x7b,
            0x4000 | if i + 1 < n { 0x0001 } else { 0 },
            &[],
            0,
        ));
        std::fs::write(t.dl().join(rar4_name(i)), v).unwrap();
    }
    let names: Vec<PathBuf> = (0..n).map(|i| t.dl().join(rar4_name(i))).collect();
    let released = Arc::new(AtomicUsize::new(1));
    let feed = Trickle {
        released: released.clone(),
        names,
        asked: Arc::new(AtomicUsize::new(0)),
    };
    let rel = released.clone();
    let releaser = std::thread::spawn(move || {
        for k in 2..=n {
            std::thread::sleep(Duration::from_millis(40));
            rel.store(k, Ordering::Release);
        }
    });
    let (r, log) = run(&t.dl().join(rar4_name(0)), &t.out(), Box::new(feed));
    releaser.join().unwrap();
    expect_ok(r, &log);
    for i in 0..n {
        assert_eq!(
            std::fs::read(t.out().join(format!("part{i}.bin"))).unwrap(),
            gold
        );
    }
}

#[test]
fn rar4_lz_damage_is_reported_not_skipped() {
    // A hole in compressed data cannot be read past: typed damage, no output.
    let t = TestDir::new("rar4lzhole");
    let gold = lz_gold();
    let mut v = b"Rar!\x1a\x07\x00".to_vec();
    v.extend(rar4_block(0x73, 0, &[0u8; 6], 0));
    let at = v.len() + rar4_lz_member("m.bin", &[], &gold, 0x33).len();
    v.extend(rar4_lz_member("m.bin", LZ[2], &gold, 0x33));
    v.extend(rar4_block(0x7b, 0x4000, &[], 0));
    let first = t.dl().join("lzhole.rar");
    std::fs::write(&first, &v).unwrap();
    struct Hole(PathBuf, u64);
    impl VolumeFeed for Hole {
        fn volume(&mut self, path: &Path) -> VolumeView {
            if path == self.0 {
                VolumeView::Ready {
                    holes: vec![(self.1, 8)],
                }
            } else {
                VolumeView::Missing
            }
        }
    }
    let (r, log) = run(
        &first,
        &t.out(),
        Box::new(Hole(first.clone(), at as u64 + 300)),
    );
    match r {
        Err(Error::Failed(f)) => assert!(f.is_damage(), "{f:?}"),
        _ => panic!("expected damage: {log:?}"),
    }
    assert!(!t.out().join("m.bin").exists());
}

/// A set with `name` whose third volume is damaged and then repaired in place (same file, so
/// the volumes can be declared protected afterwards). Returns the output dir, data and log.
fn repaired_in_place(
    t: &TestDir,
    name: &str,
    overwrite: Overwrite,
) -> (PathBuf, Vec<u8>, Vec<String>) {
    let data = pattern(600_000);
    let chunk = 100_000;
    let vols = rar5_volumes(name, &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v3 = t.dl().join(volume_name(2, vols.len()));
    let p3 = payload_offset(&vols[2], chunk) as u64;
    let mut dmg = vols[2].clone();
    dmg[(p3 + 10_000) as usize..(p3 + 30_000) as usize].fill(0);
    std::fs::write(&v3, &dmg).unwrap();
    struct InPlace(PathBuf, u64, Vec<u8>);
    impl VolumeFeed for InPlace {
        fn volume(&mut self, path: &Path) -> VolumeView {
            if path == self.0 {
                VolumeView::Ready {
                    holes: vec![(self.1, 20_000)],
                }
            } else if path.is_file() {
                VolumeView::Ready { holes: Vec::new() }
            } else {
                VolumeView::Missing
            }
        }
        fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .open(&self.0)
                .unwrap();
            f.seek(SeekFrom::Start(self.1)).unwrap();
            f.write_all(&self.2).unwrap();
            true
        }
    }
    let original = vols[2][(p3 + 10_000) as usize..(p3 + 30_000) as usize].to_vec();
    let (r, log) = run_with(
        &first,
        &t.out(),
        Box::new(InPlace(v3, p3 + 10_000, original)),
        overwrite,
    );
    expect_ok(r, &log);
    (t.out(), data, log)
}

fn moved(log: &[String]) -> bool {
    log.iter()
        .any(|l| l.contains("moved into place instead of copied"))
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn after_repair_zipfast_moves_the_patched_file_into_place() {
    let t = TestDir::new("renameafter");
    let (out, data, log) = repaired_in_place(&t, "movie.mkv", Overwrite::Always);
    assert_eq!(std::fs::read(out.join("movie.mkv")).unwrap(), data);
    assert!(moved(&log), "{log:?}");
    // The stored Unix mode, as the copy path gives it.
    #[cfg(unix)]
    assert_eq!(mode_of(&out.join("movie.mkv")), 0o644);
}

#[test]
fn a_repaired_windows_member_keeps_the_copy_path_name() {
    // zipfast would name a Windows-made member with Windows rules (a backslash splits) while the copy
    // path, which placed the rest of the set, keeps the name whole: such sets keep copying.
    RAR5_HOST.with(|h| h.set((0, 0x20)));
    let t = TestDir::new("renamewin");
    let (out, data, log) = repaired_in_place(&t, "dir\\movie.mkv", Overwrite::Always);
    RAR5_HOST.with(|h| h.set((1, 0o644)));
    assert!(!moved(&log), "{log:?}");
    let target = out.join("dir\\movie.mkv");
    assert_eq!(std::fs::read(&target).unwrap(), data);
}

#[test]
fn a_moved_member_keeps_owner_read_write() {
    RAR5_HOST.with(|h| h.set((1, 0o000)));
    let t = TestDir::new("rename000");
    let (out, _data, log) = repaired_in_place(&t, "movie.mkv", Overwrite::Always);
    RAR5_HOST.with(|h| h.set((1, 0o644)));
    assert!(moved(&log), "{log:?}");
    #[cfg(unix)]
    assert_eq!(mode_of(&out.join("movie.mkv")), 0o600);
}

#[test]
fn skip_mode_keeps_copying_after_a_repair() {
    // zipfast publishes without replacing by hard link, which exFAT and SMB lack.
    let t = TestDir::new("renameskip");
    let (out, data, log) = repaired_in_place(&t, "movie.mkv", Overwrite::Skip);
    assert_eq!(std::fs::read(out.join("movie.mkv")).unwrap(), data);
    assert!(!moved(&log), "{log:?}");
}

#[test]
fn a_complete_windows_set_gets_a_normal_mode() {
    RAR5_HOST.with(|h| h.set((0, 0x20)));
    let t = TestDir::new("completewin");
    let data = pattern(300_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    RAR5_HOST.with(|h| h.set((1, 0o644)));
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    #[cfg(unix)]
    assert_eq!(
        mode_of(&t.out().join("movie.mkv")),
        0o666 & !crate::lifecycle::active_umask() & 0o777
    );
}

/// `vols` volumes with a small `a.bin` in the first and a `movie.mkv` that spans them all, one
/// `big_mb` part in the middle; both are damaged and then repaired in place.
fn two_repairs_in_a_big_set(
    overwrite: Overwrite,
    vols: usize,
    big_mb: usize,
) -> (bool, Vec<String>, String) {
    let t = TestDir::new("tworepairs");
    let small = 64 * 1024;
    let big = big_mb << 20;
    let mut sizes = vec![small; vols];
    let k = vols / 2;
    sizes[k] = big;
    let total: usize = sizes.iter().sum();
    let data = pattern(total);
    let a: Vec<u8> = pattern(300_000).into_iter().map(|b| b ^ 0x5a).collect();
    let n = sizes.len();
    let mut at = 0;
    let mut paths = Vec::new();
    let mut a_off = 0usize;
    for (i, &sz) in sizes.iter().enumerate() {
        let part = &data[at..at + sz];
        at += sz;
        let mut out = SIG.to_vec();
        out.extend(main_block(true, if i == 0 { None } else { Some(i as u64) }));
        if i == 0 {
            let fb = file_block(
                "a.bin",
                a.len() as u64,
                &a,
                crc32fast::hash(&a),
                false,
                false,
            );
            a_off = out.len() + fb.len() - a.len();
            out.extend(fb);
        }
        let last = i + 1 == n;
        let crc = if last {
            crc32fast::hash(&data)
        } else {
            crc32fast::hash(part)
        };
        out.extend(file_block(
            "movie.mkv",
            data.len() as u64,
            part,
            crc,
            i > 0,
            !last,
        ));
        out.extend(end_block(!last));
        let p = t.dl().join(format!("set.part{:03}.rar", i + 1));
        std::fs::write(&p, &out).unwrap();
        paths.push(p);
    }
    // damage a.bin in volume 0 and movie.mkv in volume k
    let mut repairs: Vec<(PathBuf, u64, Vec<u8>)> = Vec::new();
    {
        let vol = std::fs::read(&paths[0]).unwrap();
        let off = (a_off + 100_000) as u64;
        repairs.push((
            paths[0].clone(),
            off,
            vol[off as usize..off as usize + 50_000].to_vec(),
        ));
        let mut d = vol.clone();
        d[off as usize..off as usize + 50_000].fill(0);
        std::fs::write(&paths[0], &d).unwrap();
    }
    {
        let vol = std::fs::read(&paths[k]).unwrap();
        let p = payload_offset(&vol, big) as u64 + (1 << 20);
        repairs.push((
            paths[k].clone(),
            p,
            vol[p as usize..p as usize + (1 << 20)].to_vec(),
        ));
        let mut d = vol.clone();
        d[p as usize..p as usize + (1 << 20)].fill(0);
        std::fs::write(&paths[k], &d).unwrap();
    }
    struct Feed(Vec<(PathBuf, u64, Vec<u8>)>, u64);
    impl VolumeFeed for Feed {
        fn volume(&mut self, path: &Path) -> VolumeView {
            if let Some((_, off, b)) = self.0.iter().find(|(p, _, _)| p == path) {
                VolumeView::Ready {
                    holes: vec![(*off, b.len() as u64)],
                }
            } else if path.is_file() {
                VolumeView::Ready { holes: Vec::new() }
            } else {
                VolumeView::Missing
            }
        }
        fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
            use std::io::{Seek, SeekFrom, Write};
            for (p, off, b) in &self.0 {
                let mut f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
                f.seek(SeekFrom::Start(*off)).unwrap();
                f.write_all(b).unwrap();
            }
            true
        }
        fn expected_volumes(&self) -> u64 {
            self.1
        }
    }
    let (r, log) = run_with(
        &paths[0],
        &t.out(),
        Box::new(Feed(repairs, vols as u64)),
        overwrite,
    );
    let err = match &r {
        Err(Error::Failed(f)) => format!("{f}"),
        Err(Error::Unsupported(u)) => u.0.clone(),
        Ok(_) => String::new(),
    };
    let ok = r.is_ok()
        && std::fs::read(t.out().join("movie.mkv"))
            .map(|d| d == data)
            .unwrap_or(false)
        && std::fs::read(t.out().join("a.bin"))
            .map(|d| d == a)
            .unwrap_or(false);
    (ok, log, err)
}

#[test]
fn two_repaired_outputs_keep_copying() {
    // With the volumes protected zipfast rechecks every volume against every other on each
    // read step after its first publication; for a second repaired output on a set this size
    // that exhausted its bookkeeping budget mid-verify.
    let (ok, log, err) = two_repairs_in_a_big_set(Overwrite::Always, 600, 96);
    assert!(ok, "{err}");
    assert!(!moved(&log));
}

#[test]
fn a_repair_that_replaced_a_volume_file_keeps_the_copy_path() {
    // The Damaged feed rewrites volume files with std::fs::write (same inode on most
    // filesystems), so replace the file explicitly: new inode, protection must not upgrade.
    let t = TestDir::new("replaced");
    let data = pattern(300_000);
    let chunk = 100_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let p2 = payload_offset(&vols[1], chunk) as u64;
    let mut dmg = vols[1].clone();
    dmg[(p2 + 5_000) as usize..(p2 + 6_000) as usize].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    struct Replace(PathBuf, u64, Vec<u8>);
    impl VolumeFeed for Replace {
        fn volume(&mut self, path: &Path) -> VolumeView {
            if path == self.0 {
                VolumeView::Ready {
                    holes: vec![(self.1, 1_000)],
                }
            } else if path.is_file() {
                VolumeView::Ready { holes: Vec::new() }
            } else {
                VolumeView::Missing
            }
        }
        fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
            let tmp = self.0.with_extension("new");
            std::fs::write(&tmp, &self.2).unwrap();
            std::fs::rename(&tmp, &self.0).unwrap();
            true
        }
    }
    let (r, log) = run(
        &first,
        &t.out(),
        Box::new(Replace(v2, p2 + 5_000, vols[1].clone())),
    );
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(
        !log.iter()
            .any(|l| l.contains("moved into place instead of copied")),
        "{log:?}"
    );
}

/// Timing of a post-repair patch on a large stored set (run by hand:
/// `RG_PATCH_TIMING_MB=1000 cargo test --release --lib patch_timing -- --ignored --nocapture`).
#[test]
#[ignore]
fn patch_timing() {
    let mb: usize = std::env::var("RG_PATCH_TIMING_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let t = TestDir::new("timing");
    let data = pattern(mb << 20);
    let chunk = 50 << 20;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let count = vols.len();
    let k = vols.len() / 2;
    let vk = t.dl().join(volume_name(k, vols.len()));
    let pk = payload_offset(&vols[k], chunk.min(vols[k].len())) as u64;
    let lost = (pk + (3 << 20), 2u64 << 20);
    let mut dmg = vols[k].clone();
    dmg[lost.0 as usize..(lost.0 + lost.1) as usize].fill(0);
    std::fs::write(&vk, &dmg).unwrap();
    let original = vols[k][lost.0 as usize..(lost.0 + lost.1) as usize].to_vec();
    drop(vols);
    struct InPlace(PathBuf, u64, u64, Vec<u8>, u64);
    impl VolumeFeed for InPlace {
        fn volume(&mut self, path: &Path) -> VolumeView {
            if path == self.0 {
                VolumeView::Ready {
                    holes: vec![(self.1, self.2)],
                }
            } else if path.is_file() {
                VolumeView::Ready { holes: Vec::new() }
            } else {
                VolumeView::Missing
            }
        }
        fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .open(&self.0)
                .unwrap();
            f.seek(SeekFrom::Start(self.1)).unwrap();
            f.write_all(&self.3).unwrap();
            f.sync_all().unwrap();
            true
        }
        // Direct unpack's feed knows the volume count from the queue.
        fn expected_volumes(&self) -> u64 {
            self.4
        }
    }
    let mut repaired_at = None;
    let started = std::time::Instant::now();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let r = extract(
        &req,
        Box::new(InPlace(vk, lost.0, lost.1, original, count as u64)),
        &mut |_, m: &str| {
            if m.contains("waiting for the volumes to be repaired") {
                repaired_at = Some(std::time::Instant::now());
            }
        },
        &mut || false,
    );
    let total = started.elapsed();
    expect_ok(r, &[]);
    let patch = repaired_at.map(|t0| t0.elapsed()).unwrap_or_default();
    assert_eq!(
        std::fs::metadata(t.out().join("movie.mkv")).unwrap().len(),
        data.len() as u64
    );
    eprintln!(
        "PATCH_TIMING mb={mb} total={:.2}s patch_and_publish={:.2}s",
        total.as_secs_f64(),
        patch.as_secs_f64()
    );
}

#[test]
fn protection_upgrade_is_skipped_when_the_handle_budget_cannot_fit_it() {
    // 40 volumes but the feed expects 30: the session's handle budget fits the extraction but
    // not the extra handles of the protection declaration, so the upgrade is skipped and the
    // output copied instead of failing.
    struct Under(Damaged);
    impl VolumeFeed for Under {
        fn volume(&mut self, path: &Path) -> VolumeView {
            self.0.volume(path)
        }
        fn wait_repaired(&mut self, c: &mut dyn FnMut() -> bool) -> bool {
            self.0.wait_repaired(c)
        }
        fn expected_volumes(&self) -> u64 {
            30
        }
    }
    let t = TestDir::new("budget");
    let data = pattern(40 * 4_000);
    let chunk = 4_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v = t.dl().join(volume_name(20, vols.len()));
    let p = payload_offset(&vols[20], chunk) as u64;
    let mut dmg = vols[20].clone();
    dmg[(p + 100) as usize..(p + 200) as usize].fill(0);
    std::fs::write(&v, &dmg).unwrap();
    let feed = Under(Damaged {
        holes: vec![(v.clone(), vec![(p + 100, 100)])],
        repair: vec![(v, vols[20].clone())],
        handed: false,
    });
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(
        !log.iter()
            .any(|l| l.contains("moved into place instead of copied")),
        "{log:?}"
    );
}

/// PR 430's scenario on this branch: 60 volumes, 11 damaged and repaired in place, volume count
/// known. The repaired output is moved by the session, never copied.
#[test]
fn repaired_output_with_many_damaged_volumes_is_moved_not_copied() {
    struct Counted(Damaged, u64);
    impl VolumeFeed for Counted {
        fn volume(&mut self, path: &Path) -> VolumeView {
            self.0.volume(path)
        }
        fn wait_repaired(&mut self, c: &mut dyn FnMut() -> bool) -> bool {
            self.0.wait_repaired(c)
        }
        fn expected_volumes(&self) -> u64 {
            self.1
        }
    }
    let t = TestDir::new("many_damaged_moved");
    let chunk = 50_000;
    let data = pattern(60 * chunk);
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let mut holes = Vec::new();
    let mut repair = Vec::new();
    for i in (3..vols.len() - 1).step_by(5) {
        let path = t.dl().join(volume_name(i, vols.len()));
        let p = payload_offset(&vols[i], chunk) as u64;
        let mut dmg = vols[i].clone();
        dmg[(p + 10_000) as usize..(p + 18_000) as usize].fill(0);
        std::fs::write(&path, &dmg).unwrap();
        holes.push((path.clone(), vec![(p + 10_000, 8_000)]));
        repair.push((path, vols[i].clone()));
    }
    let feed = Counted(
        Damaged {
            holes,
            repair,
            handed: false,
        },
        vols.len() as u64,
    );
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let mut log = Vec::new();
    let mut copied: Outputs = Vec::new();
    let r = extract_recording(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
        Some(&mut copied),
    );
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(moved(&log), "{log:?}");
    assert!(
        copied.is_empty(),
        "copied into place instead of moved: {copied:?}"
    );
}

/// Every volume on disk without holes, the set still "downloading" (not final at the start),
/// reported final from the start of extraction when `became` is set.
struct LateFinal {
    became: bool,
    volumes: u64,
}

impl VolumeFeed for LateFinal {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else {
            VolumeView::Missing
        }
    }
    fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
        true
    }
    fn set_became_final(&self) -> bool {
        self.became
    }
    fn expected_volumes(&self) -> u64 {
        self.volumes
    }
}

fn run_late(
    first: &Path,
    out: &Path,
    feed: Box<dyn VolumeFeed>,
    overwrite: Overwrite,
) -> (Vec<String>, Outputs) {
    let req = Request {
        first_volume: first,
        dest: out,
        password: "",
        overwrite,
        on_wait: None,
    };
    let mut log = Vec::new();
    let mut copied: Outputs = Vec::new();
    let r = extract_recording(
        &req,
        feed,
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
        Some(&mut copied),
    );
    expect_ok(r, &log);
    (log, copied)
}

#[test]
fn a_set_that_became_final_is_moved_not_copied() {
    let t = TestDir::new("latefinal");
    let data = pattern(600_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let feed = LateFinal {
        became: true,
        volumes: vols.len() as u64,
    };
    let (_log, copied) = run_late(&first, &t.out(), Box::new(feed), Overwrite::Always);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(
        copied.is_empty(),
        "copied into place instead of moved: {copied:?}"
    );
    #[cfg(unix)]
    assert_eq!(mode_of(&t.out().join("movie.mkv")), 0o644);
}

#[test]
fn a_set_still_downloading_is_copied() {
    let t = TestDir::new("latenotfinal");
    let data = pattern(600_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let feed = LateFinal {
        became: false,
        volumes: vols.len() as u64,
    };
    let (_log, copied) = run_late(&first, &t.out(), Box::new(feed), Overwrite::Always);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert_eq!(copied.len(), 1, "{copied:?}");
}

#[test]
fn a_final_set_in_skip_mode_is_copied() {
    let t = TestDir::new("lateskip");
    let data = pattern(600_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let feed = LateFinal {
        became: true,
        volumes: vols.len() as u64,
    };
    let (_log, copied) = run_late(&first, &t.out(), Box::new(feed), Overwrite::Skip);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert_eq!(copied.len(), 1, "{copied:?}");
}

#[test]
fn a_final_windows_set_keeps_the_copy_path_name() {
    RAR5_HOST.with(|h| h.set((0, 0x20)));
    let t = TestDir::new("latewin");
    let data = pattern(600_000);
    let vols = rar5_volumes("dir\\movie.mkv", &data, 100_000);
    RAR5_HOST.with(|h| h.set((1, 0o644)));
    let first = write_set(&t.dl(), &vols);
    let feed = LateFinal {
        became: true,
        volumes: vols.len() as u64,
    };
    let (_log, copied) = run_late(&first, &t.out(), Box::new(feed), Overwrite::Always);
    assert_eq!(std::fs::read(t.out().join("dir\\movie.mkv")).unwrap(), data);
    assert_eq!(copied.len(), 1, "{copied:?}");
}

#[test]
fn a_final_set_with_duplicate_names_is_copied() {
    // A later member with the same name wins; the copy path orders that itself.
    let t = TestDir::new("latedup");
    let one = pattern(50_000);
    let two: Vec<u8> = pattern(60_000).iter().map(|b| b ^ 0x33).collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block(
        "a.bin",
        one.len() as u64,
        &one,
        crc32fast::hash(&one),
        false,
        false,
    ));
    v.extend(file_block(
        "a.bin",
        two.len() as u64,
        &two,
        crc32fast::hash(&two),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("first.rar");
    std::fs::write(&first, &v).unwrap();
    let feed = LateFinal {
        became: true,
        volumes: 1,
    };
    let (_log, copied) = run_late(&first, &t.out(), Box::new(feed), Overwrite::Always);
    assert_eq!(std::fs::read(t.out().join("a.bin")).unwrap(), two);
    assert!(!copied.is_empty(), "{copied:?}");
}

// From #430: a set repaired while the session waits, then reported final. Its volumes had
// holes, so the end-of-run upgrade (#432) moves the output, not the became-final one.
/// As `Damaged`, for a download that ends and is repaired while the session waits: the feed
/// then reports the set final.
struct FinalAfterRepair {
    inner: Damaged,
    volumes: u64,
    repaired: Arc<AtomicBool>,
}

impl VolumeFeed for FinalAfterRepair {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if self.repaired.load(Ordering::Acquire) {
            return if path.is_file() {
                VolumeView::Ready { holes: Vec::new() }
            } else {
                VolumeView::Missing
            };
        }
        self.inner.volume(path)
    }
    fn wait_repaired(&mut self, is_cancelled: &mut dyn FnMut() -> bool) -> bool {
        let ok = self.inner.wait_repaired(is_cancelled);
        self.repaired.store(ok, Ordering::Release);
        ok
    }
    fn set_became_final(&self) -> bool {
        self.repaired.load(Ordering::Acquire)
    }
    fn expected_volumes(&self) -> u64 {
        self.volumes
    }
}

/// A repaired output of a set that became final is moved into place by the session, not copied
/// there byte for byte (6790: 7 min to copy a 15 GB repaired mkv). Many volumes, several of
/// them damaged, so the declared set and the repaired volumes fit the handle budget.
#[test]
fn repaired_output_of_a_final_set_is_moved_not_copied() {
    let t = TestDir::new("finalrepair");
    let chunk = 50_000;
    let data = pattern(60 * chunk);
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let mut holes = Vec::new();
    let mut repair = Vec::new();
    for i in (3..vols.len() - 1).step_by(5) {
        let path = t.dl().join(volume_name(i, vols.len()));
        let p = payload_offset(&vols[i], chunk) as u64;
        let h = vec![(p + 10_000, 8_000)];
        let mut dmg = vols[i].clone();
        dmg[(p + 10_000) as usize..(p + 18_000) as usize].fill(0);
        std::fs::write(&path, &dmg).unwrap();
        holes.push((path.clone(), h));
        repair.push((path, vols[i].clone()));
    }
    let feed = FinalAfterRepair {
        inner: Damaged {
            holes,
            repair,
            handed: false,
        },
        volumes: vols.len() as u64,
        repaired: Arc::new(AtomicBool::new(false)),
    };
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let mut log = Vec::new();
    let mut copied: Outputs = Vec::new();
    let r = extract_recording(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
        Some(&mut copied),
    );
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(
        log.iter()
            .any(|l| l.contains("repaired movie.mkv from the repaired volumes")),
        "{log:?}"
    );
    assert!(
        copied.is_empty(),
        "copied into place instead of moved: {copied:?}"
    );
}

#[test]
fn a_final_set_of_many_small_volumes_extracts() {
    // Every header step of every volume still to read rechecks the declaration; with 140
    // small volumes, declaring before the last volume exhausted zipfast's bookkeeping budget
    // mid-extraction.
    let t = TestDir::new("latesmall");
    let data = pattern(140 * 4096);
    let vols = rar5_volumes("movie.mkv", &data, 4096);
    let first = write_set(&t.dl(), &vols);
    let feed = LateFinal {
        became: true,
        volumes: vols.len() as u64,
    };
    let (_log, _copied) = run_late(&first, &t.out(), Box::new(feed), Overwrite::Always);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

/// Bytes `start..start + len` of `pattern`, without building the whole buffer.
fn pattern_at(start: usize, len: usize) -> Vec<u8> {
    (start..start + len)
        .map(|i| ((i as u64).wrapping_mul(2654435761) >> 7) as u8)
        .collect()
}

/// Writes a `volumes`-volume stored RAR5 set of `movie.mkv` (`total` bytes of `pattern`) one
/// volume at a time, so large sets need only one volume in memory. Returns the first volume,
/// the payload offset and length of each volume, and the member's CRC32.
fn write_big_set(dir: &Path, total: usize, volumes: usize) -> (PathBuf, Vec<(usize, usize)>, u32) {
    let chunk = total.div_ceil(volumes);
    let mut whole = crc32fast::Hasher::new();
    for i in 0..volumes {
        let start = i * chunk;
        whole.update(&pattern_at(start, chunk.min(total - start)));
    }
    let whole = whole.finalize();
    let mut layout = Vec::new();
    for i in 0..volumes {
        let start = i * chunk;
        let part = pattern_at(start, chunk.min(total - start));
        let last = i + 1 == volumes;
        let crc = if last { whole } else { crc32fast::hash(&part) };
        let mut out = SIG.to_vec();
        out.extend(main_block(true, if i == 0 { None } else { Some(i as u64) }));
        out.extend(file_block(
            "movie.mkv",
            total as u64,
            &part,
            crc,
            i > 0,
            !last,
        ));
        out.extend(end_block(!last));
        layout.push((payload_offset(&out, part.len()), part.len()));
        std::fs::write(dir.join(volume_name(i, volumes)), &out).unwrap();
    }
    (dir.join(volume_name(0, volumes)), layout, whole)
}

fn crc_of_file(path: &Path) -> u32 {
    use std::io::Read;
    let mut f = std::fs::File::open(path).unwrap();
    let mut h = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    h.finalize()
}

fn big_mb(default: usize) -> usize {
    std::env::var("RG_BIG_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A plain final set shaped like production job #207: 159 stored volumes through a
/// post-download unpack, declared Complete and moved into place. Before zipfast #116 the
/// declaration ran out of SourceBookkeeping at about 0.5 GB.
#[test]
fn big_final_set_unpacks() {
    let total = big_mb(1024) << 20;
    let t = TestDir::new("big_final");
    let (first, _, crc) = write_big_set(&t.dl(), total, 159);
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let mut log = Vec::new();
    let mut copied: Outputs = Vec::new();
    let r = extract_recording(
        &req,
        Box::new(CompleteSet),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
        Some(&mut copied),
    );
    expect_ok(r, &log);
    assert!(copied.is_empty(), "copied instead of moved: {copied:?}");
    assert_eq!(crc_of_file(&t.out().join("movie.mkv")), crc);
}

/// Many volumes with long paths: every volume admission sweeps the whole declared inventory,
/// so a session-start declaration of 800 volumes ran out of SourceBookkeeping where the copy
/// path works. Such sets are left undeclared and copied.
#[test]
fn a_final_set_of_many_volumes_with_long_paths_extracts() {
    let t = TestDir::new("manyvols");
    let dir = t.dl().join("x".repeat(150));
    std::fs::create_dir_all(&dir).unwrap();
    let (first, _, crc) = write_big_set(&dir, 800 * 64 * 1024, 800);
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    assert_eq!(crc_of_file(&t.out().join("movie.mkv")), crc);
}

/// Many members across many volumes: every member boundary runs a full protection refresh,
/// so a declared session of 150 volumes and 600 members ran out of SourceBookkeeping where the
/// copy path works. Such sets are left undeclared and copied.
#[test]
fn a_final_set_of_many_members_extracts() {
    let t = TestDir::new("manymembers");
    let dir = t.dl().join("y".repeat(100));
    std::fs::create_dir_all(&dir).unwrap();
    let volumes = 150;
    let per = 4;
    let mut expected = Vec::new();
    for i in 0..volumes {
        let mut v = SIG.to_vec();
        v.extend(main_block(true, if i == 0 { None } else { Some(i as u64) }));
        for j in 0..per {
            let name = format!("d{}/e{}/f{i}_{j}.bin", i % 7, j);
            let data = pattern_at(i * 10_000 + j * 997, 3_000 + j);
            v.extend(file_block(
                &name,
                data.len() as u64,
                &data,
                crc32fast::hash(&data),
                false,
                false,
            ));
            expected.push((name, data));
        }
        v.extend(end_block(i + 1 < volumes));
        std::fs::write(dir.join(volume_name(i, volumes)), &v).unwrap();
    }
    let first = dir.join(volume_name(0, volumes));
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    for (name, data) in expected {
        assert_eq!(std::fs::read(t.out().join(&name)).unwrap(), data, "{name}");
    }
}

#[test]
fn a_final_windows_set_is_copied_under_the_copy_path_name() {
    RAR5_HOST.with(|h| h.set((0, 0x20)));
    let t = TestDir::new("finalwin");
    let data = pattern(300_000);
    let vols = rar5_volumes("dir\\movie.mkv", &data, 100_000);
    RAR5_HOST.with(|h| h.set((1, 0o644)));
    let first = write_set(&t.dl(), &vols);
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("dir\\movie.mkv")).unwrap(), data);
}

/// One volume with tens of thousands of members: each declared member validation charges
/// metadata bytes, which ran out of MetadataBytes where the copy path works. A declared set
/// gets its estimated cost added to the session's limits.
#[test]
fn a_final_set_of_tens_of_thousands_of_members_extracts() {
    let t = TestDir::new("hugemembers");
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    let n = 20_000;
    let long = "n".repeat(200);
    for j in 0..n {
        let name = format!("f/{long}{j:06}.txt");
        let data = format!("{j}").into_bytes();
        v.extend(file_block(
            &name,
            data.len() as u64,
            &data,
            crc32fast::hash(&data),
            false,
            false,
        ));
    }
    v.extend(end_block(false));
    let dir = t.dl().join("z".repeat(150));
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("set.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    for j in [0, 17_000, n - 1] {
        assert_eq!(
            std::fs::read(t.out().join(format!("f/{long}{j:06}.txt"))).unwrap(),
            format!("{j}").into_bytes()
        );
    }
}

/// Many volumes with long file names in a short directory: every volume opened and every
/// publication charges each volume's name, which ran out of NameBytes where the copy path
/// works. A declared set gets that cost added to its limits too.
#[test]
fn a_final_set_of_many_long_named_volumes_extracts() {
    let t = TestDir::new("longnames");
    let volumes = 750;
    let stem = "Some.Long.Descriptive.Release.Name.2026.2160p.UHD.BluRay.REMUX.HDR.HEVC.DTS-HD.MA.7.1-GROUPNAME.and.more";
    let total = volumes * 4096;
    let chunk = 4096;
    let mut whole = crc32fast::Hasher::new();
    whole.update(&pattern_at(0, total));
    let whole = whole.finalize();
    for i in 0..volumes {
        let part = pattern_at(i * chunk, chunk);
        let last = i + 1 == volumes;
        let crc = if last { whole } else { crc32fast::hash(&part) };
        let mut out = SIG.to_vec();
        out.extend(main_block(true, if i == 0 { None } else { Some(i as u64) }));
        out.extend(file_block(
            "movie.mkv",
            total as u64,
            &part,
            crc,
            i > 0,
            !last,
        ));
        out.extend(end_block(!last));
        std::fs::write(t.dl().join(format!("{stem}.part{:03}.rar", i + 1)), &out).unwrap();
    }
    let first = t.dl().join(format!("{stem}.part001.rar"));
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    assert_eq!(crc_of_file(&t.out().join("movie.mkv")), whole);
}

/// A declared session that still runs out of a zipfast limit is extracted again undeclared:
/// forced past the estimate, 800 long-path volumes fail declared and then copy.
#[test]
fn a_declared_set_that_hits_a_limit_is_extracted_again() {
    FORCE_DECLARE.with(|f| f.set(true));
    let t = TestDir::new("retryundeclared");
    let dir = t.dl().join("x".repeat(150));
    std::fs::create_dir_all(&dir).unwrap();
    let (first, _, crc) = write_big_set(&dir, 800 * 64 * 1024, 800);
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    FORCE_DECLARE.with(|f| f.set(false));
    expect_ok(r, &log);
    assert!(
        log.iter()
            .any(|l| l.contains("extracting again without declaring")),
        "{log:?}"
    );
    assert_eq!(crc_of_file(&t.out().join("movie.mkv")), crc);
}

#[test]
fn only_limits_a_declaration_inflates_trigger_the_undeclared_retry() {
    assert!(is_declaration_limit(
        "Limit { resource: SourceBookkeeping, limit: 100663296, attempted: 100663340 } in set.part76.rar"
    ));
    assert!(is_declaration_limit(
        "Limit { resource: NameBytes, limit: 1, attempted: 2 }"
    ));
    assert!(!is_declaration_limit(
        "Limit { resource: SourceBytes, limit: 1, attempted: 2 }"
    ));
    assert!(!is_declaration_limit(
        "Limit { resource: TempBytes, limit: 1, attempted: 2 }"
    ));
    assert!(!is_declaration_limit("checksum mismatch"));
}

/// A download in progress (not final) with a known volume count and optional holes that the
/// repair fills in place, as the fleet does from par2 or a twin's articles.
struct Downloading {
    damaged: Damaged,
    volumes: u64,
}

impl VolumeFeed for Downloading {
    fn volume(&mut self, path: &Path) -> VolumeView {
        self.damaged.volume(path)
    }
    fn wait_repaired(&mut self, c: &mut dyn FnMut() -> bool) -> bool {
        self.damaged.wait_repaired(c)
    }
    fn expected_volumes(&self) -> u64 {
        self.volumes
    }
}

fn run_downloading(
    first: &Path,
    out: &Path,
    damaged: Damaged,
    volumes: u64,
) -> (Result<zipfast::extract::Summary, Error>, Vec<String>) {
    let mut log = Vec::new();
    let req = Request {
        first_volume: first,
        dest: out,
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let r = extract(
        &req,
        Box::new(Downloading { damaged, volumes }),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    (r, log)
}

/// The persistent path ran to the end (started and never fell back).
fn persistent(log: &[String]) -> bool {
    log.iter().any(|l| l.contains("persistent partial files"))
        && !log
            .iter()
            .any(|l| l.contains("progressive direct unpack ("))
}

#[test]
fn a_downloading_stored_set_extracts_into_persistent_partial_files() {
    let t = TestDir::new("persist_clean");
    let data = pattern(600_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(!t.out().join(persistent::STATE_DIR).exists());
    #[cfg(unix)]
    assert_eq!(mode_of(&t.out().join("movie.mkv")), 0o644);
}

#[test]
fn holes_in_a_persistent_output_are_refilled_from_the_repaired_volumes() {
    // A primary that died mid-download: two volumes lost ranges, which the fleet later writes
    // into the volume files. The output is never recreated; only the holes are read back.
    let t = TestDir::new("persist_holes");
    let data = pattern(600_000);
    let chunk = 100_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let mut holes = Vec::new();
    let mut repair = Vec::new();
    for i in [1usize, 3] {
        let path = t.dl().join(volume_name(i, vols.len()));
        let p = payload_offset(&vols[i], chunk) as u64;
        let mut dmg = vols[i].clone();
        dmg[(p + 10_000) as usize..(p + 30_000) as usize].fill(0);
        std::fs::write(&path, &dmg).unwrap();
        holes.push((path.clone(), vec![(p + 10_000, 20_000)]));
        repair.push((path, vols[i].clone()));
    }
    let damaged = Damaged {
        holes,
        repair,
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert!(log.iter().any(|l| l.contains("has a hole")), "{log:?}");
    assert!(
        log.iter().any(|l| l.contains("from the repaired volumes")),
        "{log:?}"
    );
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

/// A download whose volume 2 lost a range. While the session waits for volume 4, the live
/// repair writes the lost bytes back into volume 2 and releases it.
struct RepairedMidway {
    damaged: PathBuf,
    holes: Vec<(u64, u64)>,
    original: Vec<u8>,
    park_on: PathBuf,
    parks: u32,
    repaired: bool,
    waited: Arc<AtomicBool>,
    volumes: u64,
}

impl VolumeFeed for RepairedMidway {
    fn volume(&mut self, path: &Path) -> VolumeView {
        // About a second parked (the driver asks again every 50 ms).
        if path == self.park_on && self.parks < 20 {
            self.parks += 1;
            if !self.repaired {
                std::fs::write(&self.damaged, &self.original).unwrap();
                self.repaired = true;
            }
            return VolumeView::NotYet;
        }
        if path == self.damaged && !self.repaired {
            return VolumeView::Ready {
                holes: self.holes.clone(),
            };
        }
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else {
            VolumeView::Missing
        }
    }
    fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
        self.waited.store(true, Ordering::SeqCst);
        self.repaired
    }
    fn expected_volumes(&self) -> u64 {
        self.volumes
    }
    fn still_held(&self, path: &Path) -> bool {
        path == self.damaged && !self.repaired
    }
    fn repaired(&self, path: &Path) -> bool {
        path == self.damaged && self.repaired
    }
}

#[test]
fn holes_are_patched_inline_as_soon_as_their_volume_is_repaired() {
    // The user's rule: borrowed bytes go into the output while extraction runs, never after
    // the whole set. The hole in volume 2 is patched while the session is parked on volume 4,
    // before anything of volume 4 is read, and nothing waits for a repair at the end.
    let t = TestDir::new("persist_inline");
    let data = pattern(600_000);
    let chunk = 100_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let p2 = payload_offset(&vols[1], chunk) as u64;
    let mut dmg = vols[1].clone();
    dmg[(p2 + 10_000) as usize..(p2 + 30_000) as usize].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    let park_name = volume_name(3, vols.len());
    let waited = Arc::new(AtomicBool::new(false));
    let feed = RepairedMidway {
        damaged: v2,
        holes: vec![(p2 + 10_000, 20_000)],
        original: vols[1].clone(),
        park_on: t.dl().join(&park_name),
        parks: 0,
        repaired: false,
        waited: waited.clone(),
        volumes: vols.len() as u64,
    };
    let mut log = Vec::new();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let r = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    let patched = log
        .iter()
        .position(|l| l.contains("patched movie.mkv from repaired") && l.contains("20000 bytes"))
        .unwrap_or_else(|| panic!("not patched inline: {log:?}"));
    let reached = log
        .iter()
        .position(|l| l.contains("Extracting from") && l.contains(&park_name))
        .unwrap_or_else(|| panic!("volume 4 never read: {log:?}"));
    assert!(patched < reached, "patched only after volume 4: {log:?}");
    assert!(
        log.iter()
            .any(|l| l.contains("every hole was patched while extracting")),
        "{log:?}"
    );
    assert!(
        !log.iter().any(|l| l.contains("waiting for the volumes")),
        "{log:?}"
    );
    assert!(!waited.load(Ordering::SeqCst), "waited at the end: {log:?}");
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[test]
fn an_unknown_volume_count_uses_the_progressive_path() {
    let t = TestDir::new("persist_nocount");
    let data = pattern(300_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, 0);
    expect_ok(r, &log);
    assert!(!persistent(&log), "{log:?}");
    assert!(
        log.iter().any(|l| l.contains("volume count unknown")),
        "{log:?}"
    );
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

fn run_downloading_pw(
    first: &Path,
    out: &Path,
    password: &str,
    damaged: Damaged,
    volumes: u64,
) -> (Result<zipfast::extract::Summary, Error>, Vec<String>) {
    run_pw(
        first,
        out,
        password,
        Box::new(Downloading { damaged, volumes }),
    )
}

#[test]
fn a_downloading_encrypted_rar5_set_refills_its_holes() {
    // Encrypted data (AES-256-CBC), plain headers: holes mid-volume, at a volume start and
    // in the final padding, refilled across CBC neighbours.
    for (label, vol, off, len) in [
        ("mid", 1usize, 10_007u64, 2_000u64),
        ("start", 2, 0, 3_000),
        ("pad", 3, 0, 0),
    ] {
        let t = TestDir::new(&format!("persist_aes_{label}"));
        let data = pattern(320_000 - 5);
        let (vols, offs) = rar5_aes_data_volumes("movie.mkv", &data, 80_000, "pw");
        let first = write_set(&t.dl(), &vols);
        let vol = vol.min(vols.len() - 1);
        let len = if len == 0 {
            (vols[vol].len() - offs[vol]) as u64 - 16 - 8
        } else {
            len
        };
        let v = t.dl().join(volume_name(vol, vols.len()));
        let p = offs[vol] as u64 + off;
        let mut dmg = vols[vol].clone();
        dmg[p as usize..(p + len) as usize].fill(0);
        std::fs::write(&v, &dmg).unwrap();
        let damaged = Damaged {
            holes: vec![(v.clone(), vec![(p, len)])],
            repair: vec![(v, vols[vol].clone())],
            handed: false,
        };
        let (r, log) = run_downloading_pw(&first, &t.out(), "pw", damaged, vols.len() as u64);
        expect_ok(r, &log);
        assert!(persistent(&log), "{label}: {log:?}");
        assert_eq!(
            std::fs::read(t.out().join("movie.mkv")).unwrap(),
            data,
            "{label}"
        );
    }
}

/// A RAR5 set with encrypted data and plain headers.
fn rar5_aes_data_volumes(
    name: &str,
    data: &[u8],
    chunk: usize,
    password: &str,
) -> (Vec<Vec<u8>>, Vec<usize>) {
    let key = zipfast::crypto::derive_rar5_key_and_hash_key(password, 1, &RAR5_SALT, None)
        .unwrap()
        .0;
    let mut padded = data.to_vec();
    padded.resize(data.len().next_multiple_of(16), 0);
    let cipher = zipfast::crypto::encrypt_cbc(&key, &RAR5_IV, &padded).unwrap();
    let parts: Vec<&[u8]> = cipher.chunks(chunk).collect();
    let n = parts.len();
    let mut vols = Vec::new();
    let mut offsets = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == n;
        let mut v = SIG.to_vec();
        v.extend(main_block(true, (i > 0).then_some(i as u64)));
        let crc = if last {
            crc32fast::hash(data)
        } else {
            crc32fast::hash(part)
        };
        v.extend(rar5_enc_file_header(
            name,
            part.len() as u64,
            data.len() as u64,
            crc,
            i > 0,
            !last,
        ));
        offsets.push(v.len());
        v.extend_from_slice(part);
        v.extend(end_block(!last));
        vols.push(v);
    }
    (vols, offsets)
}

#[test]
fn a_downloading_rar4_stored_set_refills_its_holes() {
    let t = TestDir::new("persist_rar4");
    let data = pattern(250_000);
    let vols = rar4_store_volumes("movie.mkv", &data, 60_000);
    let first = write_named(&t.dl(), &vols, rar4_name);
    // Damage the middle of the second volume's payload.
    let v2 = t.dl().join(rar4_name(1));
    let len = vols[1].len();
    let (a, b) = (len - 30_000, len - 25_000);
    let mut dmg = vols[1].clone();
    dmg[a..b].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    let damaged = Damaged {
        holes: vec![(v2.clone(), vec![(a as u64, (b - a) as u64)])],
        repair: vec![(v2, vols[1].clone())],
        handed: false,
    };
    let (r, log) = run_downloading_pw(&first, &t.out(), "", damaged, vols.len() as u64);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn a_downloading_rar4_aes_set_extracts_in_a_persistent_session() {
    // zipfast #146: persistent sessions take RAR 3.x AES stored members.
    let t = TestDir::new("persist_rar4aes");
    let data = pattern(250_000);
    let vols = rar4_aes_store_volumes("movie.mkv", &data, 60_000, "pw");
    let first = write_named(&t.dl(), &vols, rar4_name);
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading_pw(&first, &t.out(), "pw", damaged, vols.len() as u64);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    let listing: Vec<_> = walk_names(&t.0);
    assert_eq!(
        std::fs::read(t.out().join("movie.mkv")).ok().as_deref(),
        Some(data.as_slice()),
        "{listing:?}"
    );
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

fn walk_names(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            out.push(format!(
                "{} {}",
                p.display(),
                e.metadata().map(|m| m.len()).unwrap_or(0)
            ));
            if p.is_dir() {
                out.extend(walk_names(&p));
            }
        }
    }
    out
}

#[test]
fn an_existing_output_is_replaced_as_in_overwrite_mode() {
    // The persistent session writes into its own stage; verified outputs replace what is in
    // the destination, as the copy path does in overwrite mode.
    let t = TestDir::new("persist_exists");
    let data = pattern(300_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    std::fs::write(t.out().join("movie.mkv"), b"old").unwrap();
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

/// As `big_final_set_unpacks`, for a download in progress with one lost range mid-set: the
/// persistent path stays persistent (no zipfast limit) and refills the hole.
#[test]
fn big_downloading_set_unpacks() {
    // 256 MB by default: a persistent session syncs every write, slow on macOS CI.
    let total = big_mb(256) << 20;
    let volumes = 159;
    let t = TestDir::new("big_downloading");
    let (first, layout, crc) = write_big_set(&t.dl(), total, volumes);
    let k = volumes / 2;
    let path = t.dl().join(volume_name(k, volumes));
    let good = std::fs::read(&path).unwrap();
    let at = layout[k].0 + 1000;
    let mut dmg = good.clone();
    dmg[at..at + 100_000].fill(0);
    std::fs::write(&path, &dmg).unwrap();
    let damaged = Damaged {
        holes: vec![(path.clone(), vec![(at as u64, 100_000)])],
        repair: vec![(path, good)],
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, volumes as u64);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert!(
        !log.iter()
            .any(|l| l.contains("progressive direct unpack (")),
        "{log:?}"
    );
    assert_eq!(crc_of_file(&t.out().join("movie.mkv")), crc);
}

/// Times one large stored member (RG_BIG_MB, default 15000) through the persistent path and
/// through the progressive copy path. `cargo test --release --lib big_member_timing --
/// --ignored --nocapture`.
#[test]
#[ignore]
fn big_member_timing() {
    let total = big_mb(15_000) << 20;
    let volumes: usize = std::env::var("RG_BIG_VOLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(159);
    let t = TestDir::new("big_timing");
    let (first, _, crc) = write_big_set(&t.dl(), total, volumes);
    // Skip mode keeps the progressive copy path (the persistent path needs overwrite mode)
    // with the same volume count and handle budget.
    for (label, overwrite) in [("persistent", Overwrite::Always), ("copy", Overwrite::Skip)] {
        let out = t.0.join(format!("out-{label}"));
        std::fs::create_dir_all(&out).unwrap();
        let feed = Downloading {
            damaged: Damaged {
                holes: Vec::new(),
                repair: Vec::new(),
                handed: false,
            },
            volumes: volumes as u64,
        };
        let req = Request {
            first_volume: &first,
            dest: &out,
            password: "",
            overwrite,
            on_wait: None,
        };
        let mut log = Vec::new();
        let started = std::time::Instant::now();
        let r = extract(
            &req,
            Box::new(feed),
            &mut |_, m: &str| log.push(m.to_string()),
            &mut || false,
        );
        let secs = started.elapsed().as_secs_f64();
        expect_ok(r, &log);
        assert_eq!(persistent(&log), overwrite == Overwrite::Always, "{log:?}");
        assert_eq!(crc_of_file(&out.join("movie.mkv")), crc);
        eprintln!(
            "BIG_TIMING {label} mb={} volumes={volumes} secs={secs:.1}",
            total >> 20
        );
        std::fs::remove_dir_all(&out).unwrap();
    }
}

#[test]
fn unrepaired_holes_in_a_persistent_output_leave_nothing_behind() {
    struct NoRepair(Damaged, u64);
    impl VolumeFeed for NoRepair {
        fn volume(&mut self, path: &Path) -> VolumeView {
            self.0.volume(path)
        }
        fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
            false
        }
        fn expected_volumes(&self) -> u64 {
            self.1
        }
    }
    let t = TestDir::new("persist_norepair");
    let data = pattern(600_000);
    let chunk = 100_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let path = t.dl().join(volume_name(2, vols.len()));
    let p = payload_offset(&vols[2], chunk) as u64;
    let mut dmg = vols[2].clone();
    dmg[(p + 10_000) as usize..(p + 30_000) as usize].fill(0);
    std::fs::write(&path, &dmg).unwrap();
    let feed = NoRepair(
        Damaged {
            holes: vec![(path, vec![(p + 10_000, 20_000)])],
            repair: Vec::new(),
            handed: false,
        },
        vols.len() as u64,
    );
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    assert!(
        matches!(r, Err(Error::Failed(Failure::Damaged(_)))),
        "{log:?}"
    );
    assert!(!t.out().join("movie.mkv").exists(), "{log:?}");
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[test]
fn a_downloading_member_without_checksum_extracts() {
    let t = TestDir::new("persist_nocrc");
    let data = pattern(50_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block_no_crc("a.bin", &data));
    v.extend(end_block(false));
    let first = t.dl().join("set.rar");
    std::fs::write(&first, &v).unwrap();
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, 1);
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("a.bin")).unwrap(), data);
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[test]
fn a_persistent_output_gets_the_copy_path_mode() {
    for (stored, want) in [(0o444u64, 0o644u32), (0o400, 0o600), (0o755, 0o755)] {
        RAR5_HOST.with(|h| h.set((1, stored)));
        let t = TestDir::new(&format!("persist_mode_{stored:o}"));
        let data = pattern(300_000);
        let vols = rar5_volumes("movie.mkv", &data, 100_000);
        RAR5_HOST.with(|h| h.set((1, 0o644)));
        let first = write_set(&t.dl(), &vols);
        let damaged = Damaged {
            holes: Vec::new(),
            repair: Vec::new(),
            handed: false,
        };
        let (r, log) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
        expect_ok(r, &log);
        assert!(persistent(&log), "{log:?}");
        #[cfg(unix)]
        assert_eq!(
            mode_of(&t.out().join("movie.mkv")),
            want,
            "stored {stored:o}"
        );
    }
}

// Leftover checks from the second review of the persistent path.
fn files_under(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            if p.is_dir() {
                stack.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_cancel_right_after_a_member_starts_places_nothing() {
    let t = TestDir::new("persist_cancel");
    let data = pattern(600_000);
    let vols = rar5_volumes("Release/movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let damaged = Damaged {
        holes: vec![],
        repair: vec![],
        handed: false,
    };
    let flag = std::cell::Cell::new(false);
    let mut log = Vec::new();
    let out = t.out();
    let req = Request {
        first_volume: &first,
        dest: &out,
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let r = extract(
        &req,
        Box::new(Downloading {
            damaged,
            volumes: vols.len() as u64,
        }),
        &mut |_, m: &str| {
            if m.starts_with("Unrar: Extracting  ") {
                flag.set(true);
            }
            log.push(m.to_string())
        },
        &mut || flag.get(),
    );
    // Nothing reaches the destination; the state is kept for a resume (a shutdown) and
    // removed by the direct-unpack cleanup when the job is deleted.
    let left: Vec<String> = files_under(&out)
        .into_iter()
        .filter(|p| !p.starts_with(persistent::STATE_DIR))
        .collect();
    assert!(matches!(r, Err(Error::Failed(Failure::Canceled))));
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn an_unrepaired_member_in_a_folder_leaves_nothing() {
    struct NoRepair(Damaged, u64);
    impl VolumeFeed for NoRepair {
        fn volume(&mut self, path: &Path) -> VolumeView {
            self.0.volume(path)
        }
        fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
            false
        }
        fn expected_volumes(&self) -> u64 {
            self.1
        }
    }
    let t = TestDir::new("persist_norepair_dir");
    let data = pattern(600_000);
    let chunk = 100_000;
    let vols = rar5_volumes("Release/movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let path = t.dl().join(volume_name(2, vols.len()));
    let p = payload_offset(&vols[2], chunk) as u64;
    let feed = NoRepair(
        Damaged {
            holes: vec![(path, vec![(p + 10_000, 20_000)])],
            repair: vec![],
            handed: false,
        },
        vols.len() as u64,
    );
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    assert!(
        matches!(r, Err(Error::Failed(Failure::Damaged(_)))),
        "{log:?}"
    );
    let left = files_under(&t.out());
    assert!(left.is_empty(), "{left:?}");
}

fn two_volume_set_with_host(
    host: u64,
    a: (&str, u64),
    b: (&str, u64),
    c: (&str, u64),
) -> (Vec<Vec<u8>>, [Vec<u8>; 3]) {
    let x = pattern(20_000);
    let y: Vec<u8> = pattern(30_000).iter().map(|v| v ^ 1).collect();
    let z: Vec<u8> = pattern(25_000).iter().map(|v| v ^ 2).collect();
    let set = |m: u64| RAR5_HOST.with(|h| h.set((host, m)));
    let mut v1 = SIG.to_vec();
    v1.extend(main_block(true, None));
    set(a.1);
    v1.extend(file_block(
        a.0,
        x.len() as u64,
        &x,
        crc32fast::hash(&x),
        false,
        false,
    ));
    set(b.1);
    v1.extend(file_block(
        b.0,
        y.len() as u64,
        &y[..10_000],
        crc32fast::hash(&y[..10_000]),
        false,
        true,
    ));
    v1.extend(end_block(true));
    let mut v2 = SIG.to_vec();
    v2.extend(main_block(true, Some(1)));
    v2.extend(file_block(
        b.0,
        y.len() as u64,
        &y[10_000..],
        crc32fast::hash(&y),
        true,
        false,
    ));
    set(c.1);
    v2.extend(file_block(
        c.0,
        z.len() as u64,
        &z,
        crc32fast::hash(&z),
        false,
        false,
    ));
    v2.extend(end_block(false));
    RAR5_HOST.with(|h| h.set((1, 0o644)));
    (vec![v1, v2], [x, y, z])
}

#[test]
fn a_windows_member_in_a_later_volume_leaves_no_stray_file() {
    let t = TestDir::new("persist_winbs");
    let (vols, _data) =
        two_volume_set_with_host(0, ("x.bin", 0x20), ("y.bin", 0x20), ("sub\\z.bin", 0x20));
    let first = write_set(&t.dl(), &vols);
    let damaged = Damaged {
        holes: vec![],
        repair: vec![],
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, 2);
    expect_ok(r, &log);
    let left = files_under(&t.out());
    // Reference: the same set through the progressive path only (unknown volume count).
    let t2 = TestDir::new("persist_winbs_ref");
    let first2 = write_set(&t2.dl(), &vols);
    let damaged2 = Damaged {
        holes: vec![],
        repair: vec![],
        handed: false,
    };
    let (r2, log2) = run_downloading(&first2, &t2.out(), damaged2, 0);
    expect_ok(r2, &log2);
    let left2 = files_under(&t2.out());
    assert_eq!(left, left2);
}

#[test]
fn stale_persistent_state_is_removed_when_the_path_is_not_taken() {
    let t = TestDir::new("persist_stale");
    let data = pattern(300_000);
    let vols = rar5_volumes("movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    // A crashed run's state for this set, then a post-download (final) unpack.
    persistent::plant_state_for_test(&req);
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[test]
fn a_persistent_output_in_a_folder_gets_the_copy_path_folder_mode() {
    let t = TestDir::new("persist_dirmode");
    let data = pattern(300_000);
    let vols = rar5_volumes("Release/movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(
        std::fs::read(t.out().join("Release/movie.mkv")).unwrap(),
        data
    );
    // Same folder mode as std::fs::create_dir_all gives the copy path.
    #[cfg(unix)]
    {
        let reference = t.0.join("reference");
        std::fs::create_dir_all(&reference).unwrap();
        assert_eq!(mode_of(&t.out().join("Release")), mode_of(&reference));
    }
}

#[test]
fn a_failed_persistent_run_keeps_a_pre_existing_empty_folder() {
    // Case-insensitive filesystems map "Release" onto an existing "release": nothing of the
    // run reaches the destination before verification, so it can never be removed.
    struct NoRepair(Damaged, u64);
    impl VolumeFeed for NoRepair {
        fn volume(&mut self, path: &Path) -> VolumeView {
            self.0.volume(path)
        }
        fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
            false
        }
        fn expected_volumes(&self) -> u64 {
            self.1
        }
    }
    let t = TestDir::new("persist_keepdir");
    std::fs::create_dir_all(t.out().join("release")).unwrap();
    let data = pattern(600_000);
    let chunk = 100_000;
    let vols = rar5_volumes("Release/movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let path = t.dl().join(volume_name(2, vols.len()));
    let p = payload_offset(&vols[2], chunk) as u64;
    let feed = NoRepair(
        Damaged {
            holes: vec![(path, vec![(p + 10_000, 20_000)])],
            repair: vec![],
            handed: false,
        },
        vols.len() as u64,
    );
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    assert!(
        matches!(r, Err(Error::Failed(Failure::Damaged(_)))),
        "{log:?}"
    );
    assert!(t.out().join("release").is_dir());
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[cfg(unix)]
#[test]
fn publishing_never_follows_a_symlinked_folder_in_the_destination() {
    let t = TestDir::new("persist_symlinkdir");
    let elsewhere = t.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, t.out().join("Release")).unwrap();
    let data = pattern(300_000);
    let vols = rar5_volumes("Release/movie.mkv", &data, 100_000);
    let first = write_set(&t.dl(), &vols);
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    // As the copy path: refused, and nothing written outside the destination.
    assert!(r.is_err(), "{log:?}");
    assert!(!elsewhere.join("movie.mkv").exists());
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[test]
fn a_wrong_password_after_a_refill_fails_as_the_progressive_path_does() {
    let t = TestDir::new("persist_wrongpw");
    let data = pattern(320_000 - 5);
    let (vols, offs) = rar5_aes_data_volumes("movie.mkv", &data, 80_000, "pw");
    let first = write_set(&t.dl(), &vols);
    let v = t.dl().join(volume_name(1, vols.len()));
    let p = offs[1] as u64 + 10_007;
    let mut dmg = vols[1].clone();
    dmg[p as usize..p as usize + 2_000].fill(0);
    std::fs::write(&v, &dmg).unwrap();
    let damaged = Damaged {
        holes: vec![(v.clone(), vec![(p, 2_000)])],
        repair: vec![(v, vols[1].clone())],
        handed: false,
    };
    let (r, log) = run_downloading_pw(&first, &t.out(), "wrong", damaged, vols.len() as u64);
    assert!(
        matches!(r, Err(Error::Failed(Failure::EncryptedChecksum(_)))),
        "{log:?}"
    );
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[test]
fn an_unrepaired_set_places_its_intact_members_as_the_progressive_path_does() {
    struct NoRepair(Damaged, u64);
    impl VolumeFeed for NoRepair {
        fn volume(&mut self, path: &Path) -> VolumeView {
            self.0.volume(path)
        }
        fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
            false
        }
        fn expected_volumes(&self) -> u64 {
            self.1
        }
    }
    let t = TestDir::new("persist_intact");
    let a = pattern(200_000);
    let b: Vec<u8> = pattern(50_000).iter().map(|x| x ^ 0x5a).collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    let fa = file_block(
        "a.bin",
        a.len() as u64,
        &a,
        crc32fast::hash(&a),
        false,
        false,
    );
    let a_off = v.len() + fa.len() - a.len();
    v.extend(fa);
    v.extend(file_block(
        "b.bin",
        b.len() as u64,
        &b,
        crc32fast::hash(&b),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("set.rar");
    let mut dmg = v.clone();
    dmg[a_off + 50_000..a_off + 60_000].fill(0);
    std::fs::write(&first, &dmg).unwrap();
    let feed = NoRepair(
        Damaged {
            holes: vec![(first.clone(), vec![((a_off + 50_000) as u64, 10_000)])],
            repair: Vec::new(),
            handed: false,
        },
        1,
    );
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    assert!(
        matches!(r, Err(Error::Failed(Failure::Damaged(_)))),
        "{log:?}"
    );
    assert_eq!(std::fs::read(t.out().join("b.bin")).unwrap(), b);
    assert!(!t.out().join("a.bin").exists());
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[test]
fn a_member_patched_inline_is_placed_when_another_repair_is_given_up() {
    // a.bin (volume 1) loses a range that the live repair puts back while the session waits
    // for volume 2; b.bin's hole in volume 2 is never repaired. a.bin is whole and must be
    // placed, as the progressive path places intact members, though it once had a hole.
    struct Feed {
        v1: PathBuf,
        v2: PathBuf,
        h1: Vec<(u64, u64)>,
        h2: Vec<(u64, u64)>,
        original1: Vec<u8>,
        parks: u32,
        repaired: bool,
    }
    impl VolumeFeed for Feed {
        fn volume(&mut self, path: &Path) -> VolumeView {
            if path == self.v2 {
                if self.parks < 20 {
                    self.parks += 1;
                    if !self.repaired {
                        std::fs::write(&self.v1, &self.original1).unwrap();
                        self.repaired = true;
                    }
                    return VolumeView::NotYet;
                }
                return VolumeView::Ready {
                    holes: self.h2.clone(),
                };
            }
            if path == self.v1 && !self.repaired {
                return VolumeView::Ready {
                    holes: self.h1.clone(),
                };
            }
            if path.is_file() {
                VolumeView::Ready { holes: Vec::new() }
            } else {
                VolumeView::Missing
            }
        }
        fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
            false
        }
        fn expected_volumes(&self) -> u64 {
            2
        }
        fn repaired(&self, path: &Path) -> bool {
            path == self.v1 && self.repaired
        }
    }
    let t = TestDir::new("persist_inline_giveup");
    let a = pattern(200_000);
    let b: Vec<u8> = pattern(150_000).iter().map(|x| x ^ 0x5a).collect();
    let mut vol1 = SIG.to_vec();
    vol1.extend(main_block(true, None));
    let fa = file_block("a.bin", a.len() as u64, &a, crc32fast::hash(&a), false, false);
    let a_off = vol1.len() + fa.len() - a.len();
    vol1.extend(fa);
    vol1.extend(end_block(true));
    let mut vol2 = SIG.to_vec();
    vol2.extend(main_block(true, Some(1)));
    let fb = file_block("b.bin", b.len() as u64, &b, crc32fast::hash(&b), false, false);
    let b_off = vol2.len() + fb.len() - b.len();
    vol2.extend(fb);
    vol2.extend(end_block(false));
    let first = write_set(&t.dl(), &[vol1.clone(), vol2.clone()]);
    let v2 = t.dl().join(volume_name(1, 2));
    let mut d1 = vol1.clone();
    d1[a_off + 80_000..a_off + 90_000].fill(0);
    std::fs::write(&first, &d1).unwrap();
    let mut d2 = vol2.clone();
    d2[b_off + 80_000..b_off + 90_000].fill(0);
    std::fs::write(&v2, &d2).unwrap();
    let feed = Feed {
        v1: first.clone(),
        v2,
        h1: vec![((a_off + 80_000) as u64, 10_000)],
        h2: vec![((b_off + 80_000) as u64, 10_000)],
        original1: vol1,
        parks: 0,
        repaired: false,
    };
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    assert!(
        matches!(r, Err(Error::Failed(Failure::Damaged(_)))),
        "{log:?}"
    );
    assert!(persistent(&log), "{log:?}");
    assert!(
        log.iter().any(|l| l.contains("patched a.bin from repaired")),
        "{log:?}"
    );
    assert_eq!(std::fs::read(t.out().join("a.bin")).unwrap(), a, "{log:?}");
    assert!(!t.out().join("b.bin").exists());
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

/// Like DirectUnpackFeed: direct_unpack_abandon clears held + holes, so after a give-up a
/// damaged volume is answered as Ready with no holes.
struct AbandonFeed {
    inner: Damaged,
    given_up: bool,
    expected: u64,
    waits: std::sync::Arc<std::sync::atomic::AtomicU32>,
}
impl VolumeFeed for AbandonFeed {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if self.given_up {
            return if path.is_file() {
                VolumeView::Ready { holes: Vec::new() }
            } else {
                VolumeView::Missing
            };
        }
        self.inner.volume(path)
    }
    fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
        self.waits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.given_up = true;
        false
    }
    fn expected_volumes(&self) -> u64 {
        self.expected
    }
}

fn abandon_case(
    tag: &str,
    expected: u64,
) -> (
    Result<zipfast::extract::Summary, Error>,
    Vec<String>,
    TestDir,
    Vec<u8>,
    u32,
) {
    let t = TestDir::new(tag);
    let a = pattern(200_000);
    let b: Vec<u8> = pattern(50_000).iter().map(|x| x ^ 0x5a).collect();
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    let fa = file_block(
        "a.bin",
        a.len() as u64,
        &a,
        crc32fast::hash(&a),
        false,
        false,
    );
    let a_off = v.len() + fa.len() - a.len();
    v.extend(fa);
    v.extend(file_block(
        "b.bin",
        b.len() as u64,
        &b,
        crc32fast::hash(&b),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = t.dl().join("set.rar");
    let mut dmg = v.clone();
    dmg[a_off + 50_000..a_off + 60_000].fill(0);
    std::fs::write(&first, &dmg).unwrap();
    let waits = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let feed = AbandonFeed {
        inner: Damaged {
            holes: vec![(first.clone(), vec![((a_off + 50_000) as u64, 10_000)])],
            repair: Vec::new(),
            handed: false,
        },
        given_up: false,
        expected,
        waits: waits.clone(),
    };
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    let n = waits.load(std::sync::atomic::Ordering::SeqCst);
    (r, log, t, b, n)
}

#[test]
fn a_given_up_repair_ends_as_the_progressive_path_does() {
    // expected 0: the persistent path refuses (volume count unknown), progressive path alone.
    let (r0, log0, t0, b, _) = abandon_case("persist_abandon_ref", 0);
    let (r1, log1, t1, _, _) = abandon_case("persist_abandon", 1);
    assert!(
        matches!(r0, Err(Error::Failed(Failure::Damaged(_)))),
        "{log0:?}"
    );
    assert!(
        matches!(r1, Err(Error::Failed(Failure::Damaged(_)))),
        "{log1:?}"
    );
    assert!(persistent(&log1), "{log1:?}");
    assert_eq!(std::fs::read(t0.out().join("b.bin")).unwrap(), b);
    assert_eq!(std::fs::read(t1.out().join("b.bin")).unwrap(), b);
    assert!(!t1.out().join("a.bin").exists());
    assert!(!t1.out().join(persistent::STATE_DIR).exists());
}

/// Like DirectUnpackFeed when the repair is abandoned while the session waits for a volume that
/// never arrived: the holes are forgotten and the absent volume is Missing from then on.
struct AbandonMidFeed {
    holes: Vec<(PathBuf, Vec<(u64, u64)>)>,
    abandoned: bool,
    expected: u64,
}
impl VolumeFeed for AbandonMidFeed {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if !path.is_file() {
            // The repair is given up now (direct_unpack_abandon clears held + holes).
            self.abandoned = true;
            return VolumeView::Missing;
        }
        if self.abandoned {
            return VolumeView::Ready { holes: Vec::new() };
        }
        let holes = self
            .holes
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, h)| h.clone())
            .unwrap_or_default();
        VolumeView::Ready { holes }
    }
    fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
        self.abandoned = true;
        false
    }
    fn expected_volumes(&self) -> u64 {
        self.expected
    }
}

fn abandoned_mid_set(tag: &str, expected: u64, crc: bool) -> (bool, Vec<String>, Vec<String>) {
    let t = TestDir::new(tag);
    let n = pattern(200_000);
    let c = pattern(150_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(true, None));
    let fb = if crc {
        file_block(
            "n.bin",
            n.len() as u64,
            &n,
            crc32fast::hash(&n),
            false,
            false,
        )
    } else {
        file_block_no_crc("n.bin", &n)
    };
    let off = v.len() + fb.len() - n.len();
    v.extend(fb);
    v.extend(file_block(
        "c.bin",
        c.len() as u64,
        &c[..100_000],
        crc32fast::hash(&c[..100_000]),
        false,
        true,
    ));
    v.extend(end_block(true));
    let first = t.dl().join(volume_name(0, 2));
    let hole = ((off + 80_000) as u64, 20_000u64);
    let mut dmg = v.clone();
    dmg[hole.0 as usize..(hole.0 + hole.1) as usize].fill(0);
    std::fs::write(&first, &dmg).unwrap();
    let feed = AbandonMidFeed {
        holes: vec![(first.clone(), vec![hole])],
        abandoned: false,
        expected,
    };
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    (r.is_ok(), log, files_under(&t.out()))
}

#[test]
fn a_fallback_after_a_given_up_repair_never_places_damaged_bytes() {
    // The feed forgets its holes when it gives up; the fallback must still see them.
    for crc in [false, true] {
        let (ok0, log0, l0) = abandoned_mid_set(&format!("persist_mid_ref_{crc}"), 0, crc);
        let (ok1, log1, l1) = abandoned_mid_set(&format!("persist_mid_{crc}"), 2, crc);
        assert!(!ok0 && !ok1, "{log0:?}\n{log1:?}");
        assert_eq!(l0, l1, "crc={crc}\n{log1:?}");
    }
}

/// A RAR 1.5-4.x backslash separates directories, as unrar and zipfast read it: the member
/// lands in the folder with its stored mode and archive time, in every mode.
#[cfg(unix)]
#[test]
fn a_rar4_backslash_name_lands_in_its_folder_with_mode_and_time() {
    for (tag, overwrite) in [
        ("bsalways", Overwrite::Always),
        ("bsrename", Overwrite::Rename),
        ("bsskip", Overwrite::Skip),
    ] {
        let t = TestDir::new(tag);
        let data = pattern(70_000);
        let first = write_set(
            &t.dl(),
            &rar4_store_volumes("dir\\file.bin", &data, 1 << 30),
        );
        let (r, log) = run_with(&first, &t.out(), Box::new(CompleteSet), overwrite);
        expect_ok(r, &log);
        assert!(
            log.iter().any(|l| l == "Unrar: Extracting  dir/file.bin"),
            "{tag}: {log:?}"
        );
        let out = t.out().join("dir").join("file.bin");
        assert_eq!(std::fs::read(&out).unwrap(), data, "{tag}: {log:?}");
        assert!(!t.out().join("dir\\file.bin").exists(), "{tag}");
        assert_eq!(mode_of(&out), 0o644, "{tag}");
        let age = std::time::SystemTime::now()
            .duration_since(std::fs::metadata(&out).unwrap().modified().unwrap())
            .unwrap();
        assert!(age.as_secs() > 86_400, "{tag}: archive time not restored");
    }
}

/// Skip mode checks a RAR4 backslash name where it lands.
#[test]
fn skip_mode_keeps_an_existing_rar4_backslash_target() {
    let t = TestDir::new("bsskipkept");
    let data = pattern(70_000);
    let first = write_set(
        &t.dl(),
        &rar4_store_volumes("dir\\file.bin", &data, 1 << 30),
    );
    std::fs::create_dir_all(t.out().join("dir")).unwrap();
    std::fs::write(t.out().join("dir").join("file.bin"), b"kept").unwrap();
    let (r, log) = run_with(&first, &t.out(), Box::new(CompleteSet), Overwrite::Skip);
    let s = expect_ok(r, &log);
    assert_eq!(s.skipped, 1, "{log:?}");
    assert_eq!(
        std::fs::read(t.out().join("dir").join("file.bin")).unwrap(),
        b"kept"
    );
}

/// A member the job's unpack limits refuse stops the RAR extraction before it is written.
#[test]
fn a_rar_member_over_the_unpack_limits_is_refused_before_it_is_written() {
    let t = TestDir::new("limitdeep");
    let data = pattern(70_000);
    let first = write_set(
        &t.dl(),
        &rar4_store_volumes("a\\b\\c\\file.bin", &data, 1 << 30),
    );
    let guard = crate::unpacklimits::Guard::shared(&t.out(), &[]);
    let r = {
        let _scope = crate::unpacklimits::Scope::enter(&guard);
        run_with(&first, &t.out(), Box::new(CompleteSet), Overwrite::Always)
    };
    let (r, log) = r;
    assert!(r.is_err(), "{log:?}");
    let why = crate::unpacklimits::breach_of(&guard).unwrap();
    assert!(why.contains("3 folders deep"), "{why}");
    assert!(!t.out().join("a").join("b").join("c").exists(), "{log:?}");
    // The same set passes without a guard.
    let t = TestDir::new("limitdeepok");
    let first = write_set(
        &t.dl(),
        &rar4_store_volumes("a\\b\\c\\file.bin", &data, 1 << 30),
    );
    let (r, log) = run_with(&first, &t.out(), Box::new(CompleteSet), Overwrite::Always);
    expect_ok(r, &log);
}

/// A member named outside the destination ("../") breaches the job's unpack limits with the
/// reason; nothing is written outside. (zipfast refuses the name before the member starts, so
/// the reason does not name it.)
#[test]
fn a_rar_member_with_an_unsafe_path_breaches_the_unpack_limits() {
    let t = TestDir::new("limitescape");
    let data = pattern(70_000);
    let first = write_set(
        &t.dl(),
        &rar4_store_volumes("..\\..\\escape.bin", &data, 1 << 30),
    );
    let guard = crate::unpacklimits::Guard::shared(&t.out(), &[]);
    let (r, log) = {
        let _scope = crate::unpacklimits::Scope::enter(&guard);
        run_with(&first, &t.out(), Box::new(CompleteSet), Overwrite::Always)
    };
    assert!(r.is_err(), "{log:?}");
    let why = crate::unpacklimits::breach_of(&guard).unwrap();
    assert!(
        why.starts_with(crate::unpacklimits::LIMIT_PREFIX)
            && why.contains("would land outside the unpack folder (zipfast: unsafe path"),
        "{why}"
    );
    assert!(!t.out().join("..").join("..").join("escape.bin").exists());
}

/// Dead volumes (nothing on disk) read as virtual volumes rebuilt from a sibling's headers
/// around borrowed data: the extraction runs through them as if they had arrived.
struct WithVirtual {
    virtuals: std::collections::HashMap<PathBuf, crate::deadvolume::VirtualVolume>,
}

impl VolumeFeed for WithVirtual {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if let Some(v) = self.virtuals.get(path) {
            return VolumeView::Virtual(v.clone());
        }
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else {
            VolumeView::Missing
        }
    }
}

fn geometry_of(bytes: &[u8]) -> crate::deadvolume::Geometry {
    struct Mem<'a>(&'a [u8]);
    impl crate::rarreader::Source for Mem<'_> {
        fn size(&mut self) -> i64 {
            self.0.len() as i64
        }
        fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool {
            match self.0.get(offset as usize..offset as usize + buf.len()) {
                Some(s) => {
                    buf.copy_from_slice(s);
                    true
                }
                None => false,
            }
        }
    }
    let mut vol = crate::rarreader::Volume::default();
    let mut files: Vec<crate::rarreader::RarFile> = Vec::new();
    crate::rarreader::read_from(&mut vol, &mut Mem(bytes), None, &mut files);
    let f = &files[0];
    crate::deadvolume::Geometry {
        version: vol.version,
        size: bytes.len() as i64,
        data_offset: f.data_offset,
        packed_size: f.packed_size,
        middle: f.split_before && f.split_after,
    }
}

/// Adds what real RAR5 volumes carry around the part: a locator record in the main header
/// and a quick-open service header behind the data.
fn with_locator_and_quick_open(vol: &[u8]) -> Vec<u8> {
    let mut p = SIG.len();
    let mut out = SIG.to_vec();
    let rd = |b: &[u8], p: &mut usize| -> u64 {
        let mut v = 0u64;
        let mut s = 0;
        loop {
            let c = b[*p];
            *p += 1;
            v |= u64::from(c & 0x7f) << s;
            s += 7;
            if c & 0x80 == 0 {
                return v;
            }
        }
    };
    while p < vol.len() {
        let start = p;
        p += 4;
        let size = rd(vol, &mut p) as usize;
        let body = p;
        p += size;
        let mut q = body;
        let kind = rd(vol, &mut q);
        let flags = rd(vol, &mut q);
        let data_len = if flags & 2 != 0 {
            rd(vol, &mut { q })
        } else {
            0
        };
        match kind {
            1 => {
                // main header: append an 8-byte locator extra record
                let mut fields = vol[q..body + size].to_vec();
                let mut extra = Vec::new();
                vint(7, &mut extra);
                extra.extend_from_slice(&[1, 3, 0x88, 0x0f, 0x99, 0x01, 0x00]);
                let mut b = Vec::new();
                vint(1, &mut b);
                vint(flags | 1, &mut b);
                vint(extra.len() as u64, &mut b);
                b.append(&mut fields);
                b.extend_from_slice(&extra);
                let mut sized = Vec::new();
                vint(b.len() as u64, &mut sized);
                sized.extend_from_slice(&b);
                out.extend_from_slice(&crc32fast::hash(&sized).to_le_bytes());
                out.extend_from_slice(&sized);
            }
            5 => {
                // quick-open service header with some data, then the end header
                let mut fields = Vec::new();
                vint(0, &mut fields);
                vint(58, &mut fields);
                vint(0, &mut fields);
                vint(0, &mut fields);
                vint(0, &mut fields);
                vint(2, &mut fields);
                fields.extend_from_slice(b"QO");
                out.extend(block(3, 0, &fields, &[0x5a; 58], true));
                out.extend_from_slice(&vol[start..p]);
            }
            _ => {
                out.extend_from_slice(&vol[start..p + data_len as usize]);
                p += data_len as usize;
            }
        }
    }
    out
}

#[test]
fn dead_volumes_extract_from_borrowed_data_through_rebuilt_headers() {
    use crate::deadvolume::{plan, synthesize_frame, VirtualVolume, VolumeState};
    let t = TestDir::new("deadvol");
    let data = pattern(1_000_000);
    let vols: Vec<Vec<u8>> = rar5_volumes("movie.mkv", &data, 100_000)
        .iter()
        .map(|v| with_locator_and_quick_open(v))
        .collect();
    let first = write_set(&t.dl(), &vols);
    let dead = [3usize, 4, 6];
    let states: Vec<VolumeState> = vols
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if dead.contains(&i) {
                VolumeState::Dead {
                    encoded_size: v.len() as i64 * 102 / 100,
                }
            } else {
                VolumeState::Readable(geometry_of(v))
            }
        })
        .collect();
    for &i in &dead {
        std::fs::remove_file(t.dl().join(volume_name(i, vols.len()))).unwrap();
    }
    let plans = plan(&states, data.len() as i64);
    assert_eq!(plans.iter().map(|p| p.index).collect::<Vec<_>>(), dead);
    let scratch = t.0.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let mut virtuals = std::collections::HashMap::new();
    for p in &plans {
        let borrowed = &data[p.inner_offset as usize..(p.inner_offset + p.packed_size) as usize];
        let path = scratch.join(format!("{}.data", p.index));
        std::fs::write(&path, borrowed).unwrap();
        let tv = &vols[p.template];
        let tg = geometry_of(tv);
        let (head, tail) = synthesize_frame(
            5,
            &tv[..tg.data_offset as usize],
            &tv[(tg.data_offset + tg.packed_size) as usize..],
            p.template,
            p.index,
            p.packed_size as u64,
            Some(crc32fast::hash(borrowed)), false,
        )
        .unwrap();
        crate::deadvolume::check_frame(&head, &tail, p.packed_size as u64, b"movie.mkv", false).unwrap();
        virtuals.insert(
            t.dl().join(volume_name(p.index, vols.len())),
            VirtualVolume {
                head,
                data: path,
                data_len: p.packed_size as u64,
                tail,
                holes: Vec::new(),
            },
        );
    }
    let (r, log) = run(
        &first,
        &t.out(),
        Box::new(WithVirtual {
            virtuals: virtuals.clone(),
        }),
    );
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);

    // Wrong borrowed bytes are caught by the extraction's checks, never published.
    let t2 = TestDir::new("deadvol-bad");
    let first2 = write_set(&t2.dl(), &vols);
    for &i in &dead {
        std::fs::remove_file(t2.dl().join(volume_name(i, vols.len()))).unwrap();
    }
    let mut bad = std::collections::HashMap::new();
    for (k, mut v) in virtuals {
        let name = k.file_name().unwrap().to_owned();
        let mut bytes = std::fs::read(&v.data).unwrap();
        bytes[100] ^= 1;
        let p = t2.0.join(format!("{}.bad", name.to_string_lossy()));
        std::fs::write(&p, bytes).unwrap();
        v.data = p;
        bad.insert(t2.dl().join(name), v);
    }
    let (r, log) = run(&first2, &t2.out(), Box::new(WithVirtual { virtuals: bad }));
    assert!(r.is_err(), "corrupt borrowed data must fail: {log:?}");
    assert!(!t2.out().join("movie.mkv").exists());
}

#[test]
fn a_dead_last_volume_extracts_from_borrowed_data_through_rebuilt_headers() {
    use crate::deadvolume::{plan_with_last, synthesize_frame, VirtualVolume, VolumeState};
    let t = TestDir::new("deadvol-last");
    // Eleven volumes, the last holding the file's final 50000 bytes.
    let data = pattern(1_050_000);
    let vols: Vec<Vec<u8>> = rar5_volumes("movie.mkv", &data, 100_000)
        .iter()
        .map(|v| with_locator_and_quick_open(v))
        .collect();
    let first = write_set(&t.dl(), &vols);
    let last = vols.len() - 1;
    let states: Vec<VolumeState> = vols
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if i == last {
                VolumeState::Dead { encoded_size: v.len() as i64 * 102 / 100 }
            } else {
                VolumeState::Readable(geometry_of(v))
            }
        })
        .collect();
    std::fs::remove_file(t.dl().join(volume_name(last, vols.len()))).unwrap();
    // par2 gives the dead last volume's exact length.
    let last_len = vols.last().map(|v| v.len() as i64);
    let plans = plan_with_last(&states, data.len() as i64, last_len).0;
    assert_eq!(plans.len(), 1);
    let p = plans[0];
    assert!(p.last);
    assert_eq!((p.index, p.packed_size), (last, 50_000));
    let borrowed = &data[p.inner_offset as usize..];
    let path = t.0.join("last.data");
    std::fs::write(&path, borrowed).unwrap();
    // The whole file's CRC32, as the last header stores it: the part checksums of the
    // volumes in front, chained with that of the borrowed bytes.
    let mut whole = crc32fast::Hasher::new();
    for v in &vols[..last] {
        let g = geometry_of(v);
        let crc = crate::deadvolume::rar5_part_crc(&v[..g.data_offset as usize]).unwrap();
        whole.combine(&crc32fast::Hasher::new_with_initial_len(crc, g.packed_size as u64));
    }
    let mut own = crc32fast::Hasher::new();
    own.update(borrowed);
    whole.combine(&own);
    let whole = whole.finalize();
    assert_eq!(whole, crc32fast::hash(&data));
    let tv = &vols[p.template];
    let tg = geometry_of(tv);
    let (head, tail) = synthesize_frame(
        5,
        &tv[..tg.data_offset as usize],
        &tv[(tg.data_offset + tg.packed_size) as usize..],
        p.template,
        p.index,
        p.packed_size as u64,
        Some(whole),
        true,
    )
    .unwrap();
    crate::deadvolume::check_frame(&head, &tail, p.packed_size as u64, b"movie.mkv", true).unwrap();
    // A middle volume's framing does not pass for a last one.
    assert!(crate::deadvolume::check_frame(&head, &tail, p.packed_size as u64, b"movie.mkv", false)
        .is_err());
    let volume = |data: std::path::PathBuf| {
        std::collections::HashMap::from([(
            t.dl().join(volume_name(last, vols.len())),
            VirtualVolume {
                head: head.clone(),
                data,
                data_len: p.packed_size as u64,
                tail: tail.clone(),
                holes: Vec::new(),
            },
        )])
    };
    let (r, log) = run(&first, &t.out(), Box::new(WithVirtual { virtuals: volume(path) }));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);

    // Wrong borrowed bytes fail the whole file's checksum, never published.
    let t2 = TestDir::new("deadvol-last-bad");
    let first2 = write_set(&t2.dl(), &vols);
    std::fs::remove_file(t2.dl().join(volume_name(last, vols.len()))).unwrap();
    let mut bad = borrowed.to_vec();
    bad[100] ^= 1;
    let bad_path = t2.0.join("last.bad");
    std::fs::write(&bad_path, bad).unwrap();
    let virtuals = volume(bad_path)
        .into_iter()
        .map(|(k, v)| (t2.dl().join(k.file_name().unwrap()), v))
        .collect();
    let (r, log) = run(&first2, &t2.out(), Box::new(WithVirtual { virtuals }));
    assert!(r.is_err(), "corrupt borrowed data must fail: {log:?}");
    assert!(!t2.out().join("movie.mkv").exists());
}

/// RAR4 volumes as WinRAR 3.x writes them: the end block carries the archive checksum, the
/// volume number and 7 reserved bytes (flags 0x400f, size 20).
fn rar4_winrar_volumes(name: &str, data: &[u8], chunk: usize) -> Vec<Vec<u8>> {
    rar4_store_volumes(name, data, chunk)
        .into_iter()
        .enumerate()
        .map(|(i, mut v)| {
            let more = v[v.len() - 4..v.len() - 2] == 1u16.to_le_bytes();
            v.truncate(v.len() - 7);
            let mut rest = crc32fast::hash(&v).to_le_bytes().to_vec();
            rest.extend_from_slice(&(i as u16).to_le_bytes());
            rest.extend_from_slice(&[0u8; 7]);
            let flags: u16 = 0x4000 | 0x0002 | 0x0004 | 0x0008 | u16::from(more);
            v.extend(rar4_block(0x7b, flags, &rest, 0));
            v
        })
        .collect()
}

#[test]
fn dead_rar4_volumes_extract_from_borrowed_data_through_rebuilt_headers() {
    use crate::deadvolume::{plan, synthesize_frame, VirtualVolume, VolumeState};
    let t = TestDir::new("deadvol4");
    let data = pattern(600_000);
    let vols = rar4_winrar_volumes("movie.mkv", &data, 60_000);
    // The complete set extracts: the fixture's end blocks are valid.
    let tc = TestDir::new("deadvol4-full");
    let firstc = write_named(&tc.dl(), &vols, rar4_name);
    let (r, log) = run(&firstc, &tc.out(), Box::new(CompleteSet));
    expect_ok(r, &log);

    let first = write_named(&t.dl(), &vols, rar4_name);
    let dead = [2usize, 3, 6];
    let states: Vec<VolumeState> = vols
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if dead.contains(&i) {
                VolumeState::Dead {
                    encoded_size: v.len() as i64 * 102 / 100,
                }
            } else {
                VolumeState::Readable(geometry_of(v))
            }
        })
        .collect();
    for &i in &dead {
        std::fs::remove_file(t.dl().join(rar4_name(i))).unwrap();
    }
    let plans = plan(&states, data.len() as i64);
    assert_eq!(plans.iter().map(|p| p.index).collect::<Vec<_>>(), dead);
    let mut virtuals = std::collections::HashMap::new();
    for p in &plans {
        let borrowed = &data[p.inner_offset as usize..(p.inner_offset + p.packed_size) as usize];
        let path = t.0.join(format!("{}.data", p.index));
        std::fs::write(&path, borrowed).unwrap();
        let tv = &vols[p.template];
        let tg = geometry_of(tv);
        let (head, tail) = synthesize_frame(
            tg.version,
            &tv[..tg.data_offset as usize],
            &tv[(tg.data_offset + tg.packed_size) as usize..],
            p.template,
            p.index,
            p.packed_size as u64,
            Some(crc32fast::hash(borrowed)), false,
        )
        .unwrap();
        crate::deadvolume::check_frame(&head, &tail, p.packed_size as u64, b"movie.mkv", false).unwrap();
        virtuals.insert(
            t.dl().join(rar4_name(p.index)),
            VirtualVolume {
                head,
                data: path,
                data_len: p.packed_size as u64,
                tail,
                holes: Vec::new(),
            },
        );
    }
    let (r, log) = run(&first, &t.out(), Box::new(WithVirtual { virtuals }));
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

/// One non-volume RAR4 archive of stored 1-byte members, Windows host, DOS attrs.
pub(crate) fn rar4_windows_store_archive(names: &[String]) -> Vec<u8> {
    let mut v = b"Rar!\x1a\x07\x00".to_vec();
    v.extend(rar4_block(0x73, 0, &[0u8; 6], 0));
    for name in names {
        let data = b"x";
        let mut rest = Vec::new();
        rest.extend_from_slice(&(data.len() as u32).to_le_bytes());
        rest.extend_from_slice(&(data.len() as u32).to_le_bytes());
        rest.push(2); // host OS: Windows
        rest.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
        rest.extend_from_slice(&DOS_TIME.to_le_bytes());
        rest.push(29);
        rest.push(0x30);
        rest.extend_from_slice(&(name.len() as u16).to_le_bytes());
        rest.extend_from_slice(&0x20u32.to_le_bytes());
        rest.extend_from_slice(name.as_bytes());
        v.extend(rar4_block(0x74, 0x8000, &rest, data.len()));
        v.extend_from_slice(data);
    }
    v.extend(rar4_block(0x7b, 0, &[], 0));
    v
}

/// A RAR5 directory entry.
fn dir_block(name: &str, attributes: u64, host: u64) -> Vec<u8> {
    let mut fields = Vec::new();
    vint(0x1 | 0x2, &mut fields); // directory, mtime present
    vint(0, &mut fields);
    vint(attributes, &mut fields);
    fields.extend_from_slice(&1_700_000_000u32.to_le_bytes());
    vint(0, &mut fields); // stored
    vint(host, &mut fields);
    vint(name.len() as u64, &mut fields);
    fields.extend_from_slice(name.as_bytes());
    block(2, 0, &fields, &[], true)
}

fn set_with_directory(dir: &Path, host: u64, dir_attr: u64) -> (PathBuf, Vec<u8>) {
    let a = pattern(40_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(dir_block("Sub", dir_attr, host));
    v.extend(file_block(
        "Sub/a.bin",
        a.len() as u64,
        &a,
        crc32fast::hash(&a),
        false,
        false,
    ));
    v.extend(end_block(false));
    let first = dir.join("set.rar");
    std::fs::write(&first, &v).unwrap();
    (first, a)
}

#[test]
fn a_directory_entry_extracts_while_the_set_downloads() {
    let t = TestDir::new("direntry_dl");
    let (first, a) = set_with_directory(&t.dl(), 1, 0o040550);
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    // Unknown volume count: the progressive path, undeclared.
    let (r, log) = run_downloading(&first, &t.out(), damaged, 0);
    expect_ok(r, &log);
    assert!(t.out().join("Sub").is_dir());
    assert_eq!(std::fs::read(t.out().join("Sub/a.bin")).unwrap(), a);
    // Stored 0550 with owner access kept, and the stored time.
    #[cfg(unix)]
    assert_eq!(mode_of(&t.out().join("Sub")), 0o750);
    let mtime = std::fs::metadata(t.out().join("Sub"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(mtime, 1_700_000_000);
}

#[test]
fn a_directory_entry_extracts_from_an_undeclared_complete_set() {
    // Skip mode never declares, even for a complete set.
    let t = TestDir::new("direntry_skip");
    let (first, a) = set_with_directory(&t.dl(), 0, 0x10);
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Skip,
        on_wait: None,
    };
    let mut log = Vec::new();
    let r = extract(
        &req,
        Box::new(CompleteSet),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r, &log);
    assert!(t.out().join("Sub").is_dir());
    assert_eq!(std::fs::read(t.out().join("Sub/a.bin")).unwrap(), a);
}

#[test]
fn a_windows_directory_entry_lands_where_its_files_are_copied() {
    // Windows rules would turn "What?" into "What_"; the copy path keeps the name.
    let t = TestDir::new("direntry_win_name");
    let a = pattern(30_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(dir_block("What?", 0x10, 0));
    RAR5_HOST.with(|h| h.set((0, 0x20)));
    v.extend(file_block(
        "What?/a.bin",
        a.len() as u64,
        &a,
        crc32fast::hash(&a),
        false,
        false,
    ));
    RAR5_HOST.with(|h| h.set((1, 0o644)));
    v.extend(end_block(false));
    let first = t.dl().join("set.rar");
    std::fs::write(&first, &v).unwrap();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Skip,
        on_wait: None,
    };
    let mut log = Vec::new();
    let r = extract(
        &req,
        Box::new(CompleteSet),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("What?/a.bin")).unwrap(), a);
    assert!(!t.out().join("What_").exists());
}

#[test]
fn the_persistent_switch_reads_common_values() {
    for v in ["0", "false", "OFF", " no "] {
        assert!(!persistent::switch_value(v, true), "{v}");
    }
    for v in ["1", "true", "On", "YES"] {
        assert!(persistent::switch_value(v, false), "{v}");
    }
    assert!(persistent::switch_value("", true));
    assert!(!persistent::switch_value("", false));
    assert!(!persistent::switch_value("maybe", false));
}

#[test]
fn a_first_volume_with_many_members_keeps_the_progressive_path() {
    // zipfast keeps every member's state for the session; past ~40 members its state pages
    // run out mid-set, so such sets stay on the copy path from the start.
    let t = TestDir::new("persist_manyfiles");
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    let mut files = Vec::new();
    for i in 0..31 {
        let d = pattern_at(i * 1000, 2_000);
        let name = format!("f{i:02}.bin");
        v.extend(file_block(
            &name,
            d.len() as u64,
            &d,
            crc32fast::hash(&d),
            false,
            false,
        ));
        files.push((name, d));
    }
    v.extend(end_block(false));
    let first = t.dl().join("set.rar");
    std::fs::write(&first, &v).unwrap();
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, 1);
    expect_ok(r, &log);
    assert!(
        log.iter()
            .any(|l| l.contains("members in the first volume")),
        "{log:?}"
    );
    for (name, d) in files {
        assert_eq!(std::fs::read(t.out().join(&name)).unwrap(), d);
    }
}

#[test]
fn a_restart_resumes_the_persistent_extraction_from_its_saved_state() {
    // First run: cancelled (standing in for a killed daemon) once the second volume was read,
    // its state left in place. Second run: resumes instead of starting over.
    let t = TestDir::new("persist_resume");
    let data = pattern(3_000_000);
    let vols = rar5_volumes("movie.mkv", &data, 500_000);
    let first = write_set(&t.dl(), &vols);
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let feed = Downloading {
        damaged: Damaged {
            holes: Vec::new(),
            repair: Vec::new(),
            handed: false,
        },
        volumes: vols.len() as u64,
    };
    let seen = std::cell::Cell::new(0);
    let mut log1 = Vec::new();
    let r1 = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| {
            if m.starts_with("Unrar: Extracting from ") {
                seen.set(seen.get() + 1);
            }
            log1.push(m.to_string())
        },
        &mut || seen.get() >= 4,
    );
    assert!(
        matches!(r1, Err(Error::Failed(Failure::Canceled))),
        "{log1:?}"
    );
    assert!(t.out().join(persistent::STATE_DIR).exists(), "{log1:?}");
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r2, log2) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r2, &log2);
    assert!(persistent(&log2), "{log2:?}");
    assert!(
        log2.iter().any(|l| l.contains("resuming movie.mkv")),
        "{log2:?}"
    );
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[test]
fn a_resumed_member_refills_holes_the_earlier_run_reported() {
    // First run: a hole in volume 2, cancelled before the repair (state kept). The volume is
    // repaired while the daemon is down; the second run adopts the member and refills it.
    let t = TestDir::new("persist_resume_holes");
    let data = pattern(3_000_000);
    let chunk = 500_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let p = payload_offset(&vols[1], chunk) as u64 + 100_000;
    let mut dmg = vols[1].clone();
    dmg[p as usize..p as usize + 50_000].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let feed = Downloading {
        damaged: Damaged {
            holes: vec![(v2.clone(), vec![(p, 50_000)])],
            repair: Vec::new(),
            handed: false,
        },
        volumes: vols.len() as u64,
    };
    let seen = std::cell::Cell::new(0);
    let mut log1 = Vec::new();
    let r1 = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| {
            if m.starts_with("Unrar: Extracting from ") {
                seen.set(seen.get() + 1);
            }
            log1.push(m.to_string())
        },
        &mut || seen.get() >= 4,
    );
    assert!(
        matches!(r1, Err(Error::Failed(Failure::Canceled))),
        "{log1:?}"
    );
    std::fs::write(&v2, &vols[1]).unwrap();
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r2, log2) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r2, &log2);
    assert!(
        log2.iter().any(|l| l.contains("resuming movie.mkv")),
        "{log2:?}"
    );
    assert!(
        log2.iter().any(|l| l.contains("from the repaired volumes")),
        "{log2:?}"
    );
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn a_resumed_member_with_old_and_new_holes_refills_both() {
    // First run: a hole in volume 2, cancelled before the repair (state kept). Second run: the
    // member is adopted mid-extraction, volume 2 is still damaged on disk and volume 5 reports
    // a new hole; both are repaired and both holes are refilled without leaving the persistent
    // path.
    let t = TestDir::new("persist_resume_two_holes");
    let data = pattern(3_000_000);
    let chunk = 500_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let v5 = t.dl().join(volume_name(4, vols.len()));
    let p2 = payload_offset(&vols[1], chunk) as u64 + 100_000;
    let p5 = payload_offset(&vols[4], chunk) as u64 + 100_000;
    let mut dmg2 = vols[1].clone();
    dmg2[p2 as usize..p2 as usize + 50_000].fill(0);
    std::fs::write(&v2, &dmg2).unwrap();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let feed = Downloading {
        damaged: Damaged {
            holes: vec![(v2.clone(), vec![(p2, 50_000)])],
            repair: Vec::new(),
            handed: false,
        },
        volumes: vols.len() as u64,
    };
    let seen = std::cell::Cell::new(0);
    let mut log1 = Vec::new();
    let r1 = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| {
            if m.starts_with("Unrar: Extracting from ") {
                seen.set(seen.get() + 1);
            }
            log1.push(m.to_string())
        },
        &mut || seen.get() >= 4,
    );
    assert!(
        matches!(r1, Err(Error::Failed(Failure::Canceled))),
        "{log1:?}"
    );
    let mut dmg5 = vols[4].clone();
    dmg5[p5 as usize..p5 as usize + 50_000].fill(0);
    std::fs::write(&v5, &dmg5).unwrap();
    let damaged = Damaged {
        holes: vec![(v5.clone(), vec![(p5, 50_000)])],
        repair: vec![(v2.clone(), vols[1].clone()), (v5.clone(), vols[4].clone())],
        handed: false,
    };
    let (r2, log2) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r2, &log2);
    assert!(
        log2.iter().any(|l| l.contains("resuming movie.mkv")),
        "{log2:?}"
    );
    assert!(
        log2.iter().any(|l| l.contains("from the repaired volumes")),
        "{log2:?}"
    );
    assert!(
        !log2.iter().any(|l| l.contains("progressive direct unpack (")),
        "{log2:?}"
    );
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

/// A feed after a restart: it no longer knows which volumes are damaged and repairs only the
/// ones put back on hold through `remember_damaged`.
struct Restarted {
    volumes: u64,
    remembered: Vec<PathBuf>,
    repair: Vec<(PathBuf, Vec<u8>)>,
}

impl VolumeFeed for Restarted {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if !path.is_file() {
            return VolumeView::Missing;
        }
        VolumeView::Ready { holes: Vec::new() }
    }
    fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
        for (p, bytes) in &self.repair {
            if self.remembered.contains(p) {
                std::fs::write(p, bytes).unwrap();
            }
        }
        true
    }
    fn expected_volumes(&self) -> u64 {
        self.volumes
    }
    fn remember_damaged(&mut self, path: &Path, holes: Vec<(u64, u64)>) {
        assert!(!holes.is_empty());
        self.remembered.push(path.to_path_buf());
    }
}

#[test]
fn a_restart_waits_for_the_repair_of_a_volume_the_earlier_run_saw_damaged() {
    // First run: a hole in volume 2, cancelled before the repair. The daemon restarts with the
    // volume still damaged on disk and a feed that does not know it: the saved list puts it
    // back on hold, the refill waits for its repair and the set never leaves the persistent
    // path.
    let t = TestDir::new("persist_resume_pending_repair");
    let data = pattern(3_000_000);
    let chunk = 500_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let p = payload_offset(&vols[1], chunk) as u64 + 100_000;
    let mut dmg = vols[1].clone();
    dmg[p as usize..p as usize + 50_000].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let feed = Downloading {
        damaged: Damaged {
            holes: vec![(v2.clone(), vec![(p, 50_000)])],
            repair: Vec::new(),
            handed: false,
        },
        volumes: vols.len() as u64,
    };
    let seen = std::cell::Cell::new(0);
    let mut log1 = Vec::new();
    let r1 = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| {
            if m.starts_with("Unrar: Extracting from ") {
                seen.set(seen.get() + 1);
            }
            log1.push(m.to_string())
        },
        &mut || seen.get() >= 4,
    );
    assert!(
        matches!(r1, Err(Error::Failed(Failure::Canceled))),
        "{log1:?}"
    );
    let mut log2 = Vec::new();
    let r2 = extract(
        &req,
        Box::new(Restarted {
            volumes: vols.len() as u64,
            remembered: Vec::new(),
            repair: vec![(v2.clone(), vols[1].clone())],
        }),
        &mut |_, m: &str| log2.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r2, &log2);
    assert!(
        log2.iter()
            .any(|l| l.contains("still waiting for its repair")),
        "{log2:?}"
    );
    assert!(persistent(&log2), "{log2:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(!persistent_state_left(&t.out()), "state left behind");
}

#[test]
fn the_pending_repair_list_round_trips_and_ignores_bad_lines() {
    let t = TestDir::new("persist_pending_repair_file");
    let file = t.out().join("pending-repair");
    std::fs::create_dir_all(t.out()).unwrap();
    let mut map = std::collections::HashMap::new();
    map.insert(t.dl().join("a.part02.rar"), vec![(10, 20), (100, 5)]);
    map.insert(t.dl().join("a.part05.rar"), vec![(0, 7)]);
    persistent::save_pending_repair(&file, &map);
    let mut back = persistent::load_pending_repair(&file, &t.dl());
    back.sort();
    let mut want: Vec<_> = map.into_iter().collect();
    want.sort();
    assert_eq!(back, want);
    std::fs::write(
        &file,
        b"1:2\tok.rar\nx:y\tbad.rar\n3:4\t../up.rar\n\t\n5:6\n7:8\tsub/dir.rar\n",
    )
    .unwrap();
    assert_eq!(
        persistent::load_pending_repair(&file, &t.dl()),
        vec![(t.dl().join("ok.rar"), vec![(1, 2)])]
    );
    persistent::save_pending_repair(&file, &Default::default());
    assert!(!file.exists());
}

fn persistent_state_left(out: &Path) -> bool {
    out.join(super::PERSISTENT_STATE_DIR).exists()
}

/// `Downloading` whose damaged volumes count as repaired and released once handed out.
struct ReleasedAfterHanding {
    inner: Downloading,
}

impl VolumeFeed for ReleasedAfterHanding {
    fn volume(&mut self, path: &Path) -> VolumeView {
        self.inner.volume(path)
    }
    fn wait_repaired(&mut self, c: &mut dyn FnMut() -> bool) -> bool {
        self.inner.wait_repaired(c)
    }
    fn expected_volumes(&self) -> u64 {
        self.inner.expected_volumes()
    }
    fn still_held(&self, _path: &Path) -> bool {
        false
    }
}

fn saved_pending_repair(out: &Path) -> Vec<u8> {
    let root = out.join(super::PERSISTENT_STATE_DIR);
    std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path().join("pending-repair")).ok())
        .next()
        .unwrap_or_default()
}

#[test]
fn a_damaged_volume_released_before_a_restart_is_not_saved_for_it() {
    // Volume 2 is handed out with a hole, then repaired and released while the session reads
    // on: the next volume request drops it from the saved list, so a restart would not hold
    // it again. The same set with the hold kept saves it.
    for released in [false, true] {
        let t = TestDir::new(&format!("persist_pending_released_{released}"));
        let data = pattern(3_000_000);
        let chunk = 500_000;
        let vols = rar5_volumes("movie.mkv", &data, chunk);
        let first = write_set(&t.dl(), &vols);
        let v2 = t.dl().join(volume_name(1, vols.len()));
        let p = payload_offset(&vols[1], chunk) as u64 + 100_000;
        let req = Request {
            first_volume: &first,
            dest: &t.out(),
            password: "",
            overwrite: Overwrite::Always,
            on_wait: None,
        };
        let inner = Downloading {
            damaged: Damaged {
                holes: vec![(v2.clone(), vec![(p, 50_000)])],
                repair: Vec::new(),
                handed: false,
            },
            volumes: vols.len() as u64,
        };
        let feed: Box<dyn VolumeFeed> = if released {
            Box::new(ReleasedAfterHanding { inner })
        } else {
            Box::new(inner)
        };
        let seen = std::cell::Cell::new(0);
        let mut log = Vec::new();
        let r = extract(
            &req,
            feed,
            &mut |_, m: &str| {
                if m.starts_with("Unrar: Extracting from ") {
                    seen.set(seen.get() + 1);
                }
                log.push(m.to_string())
            },
            &mut || seen.get() >= 4,
        );
        assert!(matches!(r, Err(Error::Failed(Failure::Canceled))), "{log:?}");
        let saved = String::from_utf8_lossy(&saved_pending_repair(&t.out())).into_owned();
        let name = v2.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(saved.contains(&name), !released, "released={released}: {saved:?}");
    }
}

/// Volume 2 is handed out with a hole and released right after; volume 3 is not there the
/// first time it is asked for.
struct ReleasedThenNotYet {
    inner: Downloading,
    v2: PathBuf,
    v3: PathBuf,
    released: std::sync::atomic::AtomicBool,
    v3_asked: bool,
}

impl VolumeFeed for ReleasedThenNotYet {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path == self.v3 && !self.v3_asked {
            self.v3_asked = true;
            return VolumeView::NotYet;
        }
        let view = self.inner.volume(path);
        if path == self.v2 {
            self.released.store(true, Ordering::Release);
        }
        view
    }
    fn wait_repaired(&mut self, c: &mut dyn FnMut() -> bool) -> bool {
        self.inner.wait_repaired(c)
    }
    fn expected_volumes(&self) -> u64 {
        self.inner.expected_volumes()
    }
    fn still_held(&self, path: &Path) -> bool {
        !(path == self.v2 && self.released.load(Ordering::Acquire))
    }
}

#[test]
fn a_release_seen_while_the_next_volume_is_not_there_is_saved() {
    let t = TestDir::new("persist_pending_released_notyet");
    let data = pattern(3_000_000);
    let chunk = 500_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let v3 = t.dl().join(volume_name(2, vols.len()));
    let p = payload_offset(&vols[1], chunk) as u64 + 100_000;
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let feed = ReleasedThenNotYet {
        inner: Downloading {
            damaged: Damaged {
                holes: vec![(v2.clone(), vec![(p, 50_000)])],
                repair: Vec::new(),
                handed: false,
            },
            volumes: vols.len() as u64,
        },
        v2: v2.clone(),
        v3,
        released: std::sync::atomic::AtomicBool::new(false),
        v3_asked: false,
    };
    let seen = std::cell::Cell::new(0);
    let mut log = Vec::new();
    let r = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| {
            if m.starts_with("Unrar: Extracting from ") {
                seen.set(seen.get() + 1);
            }
            log.push(m.to_string())
        },
        &mut || seen.get() >= 4,
    );
    assert!(matches!(r, Err(Error::Failed(Failure::Canceled))), "{log:?}");
    let saved = String::from_utf8_lossy(&saved_pending_repair(&t.out())).into_owned();
    let name = v2.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!saved.contains(&name), "{saved:?} {log:?}");
}

/// A downloading set whose dead volumes are borrowed while the session waits: the feed says
/// NotYet `late` times for each dead volume, then hands the rebuilt virtual volume.
struct DownloadingWithVirtual {
    virtuals: std::collections::HashMap<PathBuf, crate::deadvolume::VirtualVolume>,
    late: usize,
    asked: std::collections::HashMap<PathBuf, usize>,
    volumes: u64,
}

impl VolumeFeed for DownloadingWithVirtual {
    fn has_borrowed(&self) -> bool {
        !self.virtuals.is_empty()
    }
    fn volume(&mut self, path: &Path) -> VolumeView {
        if let Some(v) = self.virtuals.get(path) {
            let n = self.asked.entry(path.to_path_buf()).or_default();
            *n += 1;
            if *n <= self.late {
                return VolumeView::NotYet;
            }
            return VolumeView::Virtual(v.clone());
        }
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else {
            VolumeView::Missing
        }
    }
    fn expected_volumes(&self) -> u64 {
        self.volumes
    }
}

/// A 10-volume set whose volumes 3, 4 and 5 are dead and rebuilt from borrowed data: the
/// first volume, the inner file, the virtual volumes and the volume count.
fn borrowed_run_set(
    t: &TestDir,
) -> (
    PathBuf,
    Vec<u8>,
    std::collections::HashMap<PathBuf, crate::deadvolume::VirtualVolume>,
    u64,
) {
    use crate::deadvolume::{plan, synthesize_frame, VirtualVolume, VolumeState};
    let data = pattern(1_000_000);
    let vols: Vec<Vec<u8>> = rar5_volumes("movie.mkv", &data, 100_000)
        .iter()
        .map(|v| with_locator_and_quick_open(v))
        .collect();
    let first = write_set(&t.dl(), &vols);
    let dead = [3usize, 4, 5];
    let states: Vec<VolumeState> = vols
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if dead.contains(&i) {
                VolumeState::Dead {
                    encoded_size: v.len() as i64 * 102 / 100,
                }
            } else {
                VolumeState::Readable(geometry_of(v))
            }
        })
        .collect();
    for &i in &dead {
        std::fs::remove_file(t.dl().join(volume_name(i, vols.len()))).unwrap();
    }
    let plans = plan(&states, data.len() as i64);
    assert_eq!(plans.iter().map(|p| p.index).collect::<Vec<_>>(), dead);
    let scratch = t.0.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let mut virtuals = std::collections::HashMap::new();
    for p in &plans {
        let borrowed = &data[p.inner_offset as usize..(p.inner_offset + p.packed_size) as usize];
        let path = scratch.join(format!("{}.data", p.index));
        std::fs::write(&path, borrowed).unwrap();
        let tv = &vols[p.template];
        let tg = geometry_of(tv);
        let (head, tail) = synthesize_frame(
            5,
            &tv[..tg.data_offset as usize],
            &tv[(tg.data_offset + tg.packed_size) as usize..],
            p.template,
            p.index,
            p.packed_size as u64,
            Some(crc32fast::hash(borrowed)), false,
        )
        .unwrap();
        virtuals.insert(
            t.dl().join(volume_name(p.index, vols.len())),
            VirtualVolume {
                head,
                data: path,
                data_len: p.packed_size as u64,
                tail,
                holes: Vec::new(),
            },
        );
    }
    (first, data, virtuals, vols.len() as u64)
}

#[test]
fn a_persistent_session_reads_a_run_of_borrowed_volumes() {
    for late in [0usize, 3] {
        let t = TestDir::new(&format!("deadvol-persist-{late}"));
        let (first, data, virtuals, volumes) = borrowed_run_set(&t);
        let feed = DownloadingWithVirtual {
            virtuals,
            late,
            asked: Default::default(),
            volumes,
        };
        let (r, log) = run(&first, &t.out(), Box::new(feed));
        expect_ok(r, &log);
        assert!(persistent(&log), "late {late}: {log:?}");
        assert_eq!(
            std::fs::read(t.out().join("movie.mkv")).unwrap(),
            data,
            "late {late}"
        );
    }
}

/// [`DownloadingWithVirtual`] whose download has ended: the set is final while the session
/// still waits on the borrowed volumes.
struct FinalWithVirtual(DownloadingWithVirtual);

impl VolumeFeed for FinalWithVirtual {
    fn volume(&mut self, path: &Path) -> VolumeView {
        self.0.volume(path)
    }
    fn expected_volumes(&self) -> u64 {
        // Unknown, as when the queue cannot count the set: the guard must not depend on it.
        0
    }
    fn set_became_final(&self) -> bool {
        true
    }
    fn has_borrowed(&self) -> bool {
        self.0.has_borrowed()
    }
}

#[test]
fn the_copy_path_reads_borrowed_volumes_after_the_download_ends() {
    // The progressive (non-persistent) session declares a set that became final: the set on
    // disk stops at the first dead volume, so that declaration would leave the borrowed
    // volumes out and reject them (SourceContract).
    for late in [0usize, 3] {
        let t = TestDir::new(&format!("deadvol-final-{late}"));
        let (first, data, virtuals, volumes) = borrowed_run_set(&t);
        let feed = FinalWithVirtual(DownloadingWithVirtual {
            virtuals,
            late,
            asked: Default::default(),
            volumes,
        });
        let out = t.out();
        let req = Request {
            first_volume: &first,
            dest: &out,
            password: "",
            overwrite: Overwrite::Always,
            on_wait: None,
        };
        let mut log = Vec::new();
        let mut declared = false;
        let r = extract_once(
            &req,
            Box::new(feed),
            &mut |_, m: &str| log.push(m.to_string()),
            &mut || false,
            None,
            true,
            &mut declared,
        );
        expect_ok(r, &log);
        assert_eq!(
            std::fs::read(t.out().join("movie.mkv")).unwrap(),
            data,
            "late {late}"
        );
    }
}

/// A RAR5 file header for a middle part with no part CRC (flag 0x4 clear), as a dead
/// volume's rebuilt headers carry while some of its bytes are not borrowed yet.
fn file_block_no_part_crc(name: &str, size: u64, part: &[u8]) -> Vec<u8> {
    let (host, attributes) = RAR5_HOST.with(|h| h.get());
    let mut fields = Vec::new();
    vint(0x2, &mut fields); // mtime only
    vint(size, &mut fields);
    vint(attributes, &mut fields);
    fields.extend_from_slice(&1_700_000_000u32.to_le_bytes());
    vint(0, &mut fields);
    vint(host, &mut fields);
    vint(name.len() as u64, &mut fields);
    fields.extend_from_slice(name.as_bytes());
    block(2, 0x8 | 0x10, &fields, part, true)
}

#[test]
fn probe_middle_part_without_crc_with_hole_refills_and_verifies() {
    let t = TestDir::new("probe_nocrc_mid");
    let data = pattern(600_000);
    let chunk = 100_000;
    let mut vols = rar5_volumes("movie.mkv", &data, chunk);
    // Volume 3 rebuilt without a part CRC.
    let mut v = SIG.to_vec();
    v.extend(main_block(true, Some(2)));
    v.extend(file_block_no_part_crc("movie.mkv", data.len() as u64, &data[200_000..300_000]));
    v.extend(end_block(true));
    vols[2] = v;
    let first = write_set(&t.dl(), &vols);
    let path = t.dl().join(volume_name(2, vols.len()));
    let p = payload_offset(&vols[2], chunk) as u64;
    let mut dmg = vols[2].clone();
    dmg[(p + 10_000) as usize..(p + 30_000) as usize].fill(0);
    std::fs::write(&path, &dmg).unwrap();
    let damaged = Damaged {
        holes: vec![(path.clone(), vec![(p + 10_000, 20_000)])],
        repair: vec![(path, vols[2].clone())],
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

/// A download whose volume 4 died: its stand-in is handed over while only its first half is
/// borrowed (the rest are holes). The borrow finishes while the session waits for volume 7.
struct DeadHalfBorrowed {
    dead: PathBuf,
    holed: crate::deadvolume::VirtualVolume,
    rest: Vec<u8>,
    rest_at: u64,
    park_on: PathBuf,
    parks: u32,
    finished: bool,
    asked_holed: bool,
    volumes: u64,
    /// Finishes the borrow once this long has passed since the stand-in was first asked for.
    finish_after: Option<(std::time::Duration, Option<std::time::Instant>)>,
    /// How often the stand-in was asked for with holes.
    holed_asks: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl VolumeFeed for DeadHalfBorrowed {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path == self.park_on && self.parks < 20 {
            self.parks += 1;
            if !self.finished {
                use std::os::unix::fs::FileExt;
                let f = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&self.holed.data)
                    .unwrap();
                f.write_all_at(&self.rest, self.rest_at).unwrap();
                self.finished = true;
            }
            return VolumeView::NotYet;
        }
        if path == self.dead {
            if let Some((after, since)) = &mut self.finish_after {
                let since = *since.get_or_insert_with(std::time::Instant::now);
                if !self.finished && since.elapsed() >= *after {
                    use std::os::unix::fs::FileExt;
                    let f = std::fs::OpenOptions::new()
                        .write(true)
                        .open(&self.holed.data)
                        .unwrap();
                    f.write_all_at(&self.rest, self.rest_at).unwrap();
                    self.finished = true;
                }
            }
            let mut v = self.holed.clone();
            if self.finished {
                v.holes.clear();
            } else {
                self.asked_holed = true;
                self.holed_asks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            return VolumeView::Virtual(v);
        }
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else {
            VolumeView::Missing
        }
    }
    fn wait_repaired(&mut self, _c: &mut dyn FnMut() -> bool) -> bool {
        self.finished
    }
    fn expected_volumes(&self) -> u64 {
        self.volumes
    }
    fn has_borrowed(&self) -> bool {
        true
    }
    fn still_held(&self, path: &Path) -> bool {
        path == self.dead && !self.finished
    }
    fn repaired(&self, path: &Path) -> bool {
        path == self.dead && self.finished
    }
}

/// Builds the half-borrowed dead volume 4 of a 10-volume RAR5 set, and the feed around it.
fn half_borrowed_dead_volume(t: &TestDir, park_on: usize) -> (PathBuf, Vec<u8>, DeadHalfBorrowed) {
    borrowed_dead_volume(t, park_on, 2)
}

/// As [`half_borrowed_dead_volume`], with `1/part` of the dead volume borrowed (0: none).
fn borrowed_dead_volume(t: &TestDir, park_on: usize, part: usize) -> (PathBuf, Vec<u8>, DeadHalfBorrowed) {
    borrowed_dead_volume_of(t, park_on, part, 5)
}

/// As [`borrowed_dead_volume`], in a RAR5 (`version` 5) or RAR 1.5-4 (3) set.
fn borrowed_dead_volume_of(
    t: &TestDir,
    park_on: usize,
    part: usize,
    version: i32,
) -> (PathBuf, Vec<u8>, DeadHalfBorrowed) {
    use crate::deadvolume::{plan, synthesize_frame, VirtualVolume, VolumeState};
    let data = pattern(1_000_000);
    let vols = if version == 5 {
        rar5_volumes("movie.mkv", &data, 100_000)
    } else {
        rar4_store_volumes("movie.mkv", &data, 100_000)
    };
    let first = write_set(&t.dl(), &vols);
    let dead = 3usize;
    let states: Vec<VolumeState> = vols
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if i == dead {
                VolumeState::Dead {
                    encoded_size: v.len() as i64 * 102 / 100,
                }
            } else {
                VolumeState::Readable(geometry_of(v))
            }
        })
        .collect();
    let dead_path = t.dl().join(volume_name(dead, vols.len()));
    std::fs::remove_file(&dead_path).unwrap();
    let p = plan(&states, data.len() as i64)
        .into_iter()
        .find(|p| p.index == dead)
        .unwrap();
    let borrowed = &data[p.inner_offset as usize..(p.inner_offset + p.packed_size) as usize];
    let half = if part == 0 { 0 } else { borrowed.len() / part };
    let scratch = t.0.join("deadvol.data");
    let mut first_half = borrowed[..half].to_vec();
    first_half.resize(borrowed.len(), 0);
    std::fs::write(&scratch, &first_half).unwrap();
    let tv = &vols[p.template];
    let tg = geometry_of(tv);
    let (head, tail) = synthesize_frame(
        version,
        &tv[..tg.data_offset as usize],
        &tv[(tg.data_offset + tg.packed_size) as usize..],
        p.template,
        p.index,
        p.packed_size as u64,
        None, false,
    )
    .unwrap();
    let at = head.len() as u64;
    let holed = VirtualVolume {
        head,
        data: scratch,
        data_len: p.packed_size as u64,
        tail,
        holes: vec![(at + half as u64, (borrowed.len() - half) as u64)],
    };
    let feed = DeadHalfBorrowed {
        dead: dead_path,
        holed,
        rest: borrowed[half..].to_vec(),
        rest_at: half as u64,
        park_on: t.dl().join(volume_name(park_on, vols.len())),
        parks: 0,
        finished: false,
        asked_holed: false,
        volumes: vols.len() as u64,
        finish_after: None,
        holed_asks: Default::default(),
    };
    (first, data, feed)
}

#[test]
fn a_half_borrowed_dead_volume_is_extracted_past_and_patched_inline() {
    // The user's goal: never wait at a dead volume. Extraction reads volume 4 while half of it
    // is still being borrowed, goes on to volumes 5 and 6, and the rest is patched in from the
    // finished stand-in while the session waits for volume 7, before volume 7 is read.
    let t = TestDir::new("deadvol-half");
    let (first, data, feed) = half_borrowed_dead_volume(&t, 6);
    let mut log = Vec::new();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let r = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    let pos = |needle: &str| {
        log.iter()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?}: {log:?}"))
    };
    let past = pos("Extracting from set.part06.rar");
    let patched = pos("patched movie.mkv from repaired set.part04.rar while extracting");
    let reached = pos("Extracting from set.part07.rar");
    assert!(past < patched && patched < reached, "{log:?}");
    assert!(
        log.iter()
            .any(|l| l.contains("every hole was patched while extracting")),
        "{log:?}"
    );
}

#[test]
fn a_dead_volume_handed_over_before_any_byte_is_borrowed_is_extracted_past() {
    // On 6790 a whole dead volume was handed over with nothing borrowed yet: extraction must
    // read it as one hole and go on, not spin on it.
    let t = TestDir::new("deadvol-none");
    let (first, data, feed) = borrowed_dead_volume(&t, 6, 0);
    let mut log = Vec::new();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let r = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn the_copy_path_parks_on_a_holed_stand_in_without_spinning() {
    // 6790: the copy path waited on a stand-in handed over with holes, but the driver woke it
    // again at once each time (the stand-in was no longer "not yet"), until zipfast's wake
    // limit failed the extraction. It must park until the stand-in is finished.
    let t = TestDir::new("deadvol-copy-park");
    let (first, data, mut feed) = borrowed_dead_volume(&t, 9, 0);
    feed.finish_after = Some((std::time::Duration::from_millis(600), None));
    let asks = feed.holed_asks.clone();
    let (r, log) = run_with(&first, &t.out(), Box::new(feed), Overwrite::Skip);
    expect_ok(r, &log);
    assert!(!persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    let asks = asks.load(std::sync::atomic::Ordering::Relaxed);
    assert!(asks < 200, "asked for the holed stand-in {asks} times in 600 ms");
}

#[test]
fn a_half_borrowed_rar4_dead_volume_is_extracted_past_and_patched_inline() {
    // RAR 1.5-4 parts carry their own checksum, unknown until every byte is borrowed: the
    // stand-in carries 0 and the session skips that check (`rar4_unknown_part_crc`), so the
    // extraction goes past it too and the member checksum checks the patched file.
    let t = TestDir::new("deadvol-half-rar4");
    let (first, data, feed) = borrowed_dead_volume_of(&t, 6, 2, 3);
    let mut log = Vec::new();
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let r = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
    );
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(
        log.iter().any(|l| l.contains("patched movie.mkv from repaired")),
        "{log:?}"
    );
}

#[test]
fn the_copy_path_waits_for_a_half_borrowed_dead_volume_to_finish() {
    // Skip mode takes the progressive copy path, which patches only from volumes on disk: it
    // waits at the dead volume until the stand-in is finished, as before.
    let t = TestDir::new("deadvol-half-copy");
    let (first, data, mut feed) = half_borrowed_dead_volume(&t, 3);
    // Parking on the dead volume itself finishes its borrow.
    feed.park_on = feed.dead.clone();
    let (r, log) = run_with(&first, &t.out(), Box::new(feed), Overwrite::Skip);
    expect_ok(r, &log);
    assert!(!persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}


/// A volume whose end block (and the quick-open record in front of it) was lost and rebuilt
/// from a sibling (`deadvolume::restore_tail_from_sibling`) still extracts: the sibling's
/// quick-open record describes the sibling's part, which the extraction does not read.
#[test]
fn a_volume_with_its_end_rebuilt_from_a_sibling_extracts() {
    let t = TestDir::new("sibling-tail");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/testdata/rarrenamer");
    for i in 1..=3 {
        let name = format!("testfile5.part0{i}.rar");
        std::fs::copy(src.join(&name), t.dl().join(&name)).unwrap();
    }
    let two = t.dl().join("testfile5.part02.rar");
    let one = t.dl().join("testfile5.part01.rar");
    let mut v2 = std::fs::read(&two).unwrap();
    let v1 = std::fs::read(&one).unwrap();
    assert_ne!(v1[10_146..], v2[10_146..]);
    let n = v2.len();
    v2[10_146..].fill(0);
    std::fs::write(&two, &v2).unwrap();
    let path = |p: &Path| p.to_str().unwrap().as_bytes().to_vec();
    assert!(crate::deadvolume::restore_tail_from_sibling(
        &path(&two),
        &[(10_146, n as u64)],
        &path(&one)
    ));
    let (r, log) = run(&one, &t.out(), Box::new(CompleteSet));
    expect_ok(r, &log);
    assert!(t.out().join("testfile5.dat").is_file(), "{log:?}");
}

#[test]
fn a_directory_entry_keeps_the_persistent_session() {
    // Before, a directory entry made the persistent session fall back and extract the whole set
    // again; it is now made at publish with its stored mode and time.
    let t = TestDir::new("direntry_persist");
    let (first, a) = set_with_directory(&t.dl(), 1, 0o040550);
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, 1);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("Sub/a.bin")).unwrap(), a);
    #[cfg(unix)]
    assert_eq!(mode_of(&t.out().join("Sub")), 0o750);
    let mtime = std::fs::metadata(t.out().join("Sub"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(mtime, 1_700_000_000);
    assert!(!t.out().join(persistent::STATE_DIR).exists());
}

#[test]
fn a_trailing_empty_directory_entry_keeps_the_persistent_session() {
    // Scene releases end with their folder's entry; an empty one must survive publishing.
    let t = TestDir::new("direntry_persist_tail");
    let a = pattern(40_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block(
        "Rel/a.bin",
        a.len() as u64,
        &a,
        crc32fast::hash(&a),
        false,
        false,
    ));
    v.extend(dir_block("Rel/Empty", 0o040755, 1));
    v.extend(dir_block("Rel", 0o040755, 1));
    v.extend(end_block(false));
    let first = t.dl().join("set.rar");
    std::fs::write(&first, &v).unwrap();
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r, log) = run_downloading(&first, &t.out(), damaged, 1);
    expect_ok(r, &log);
    assert!(persistent(&log), "{log:?}");
    assert_eq!(std::fs::read(t.out().join("Rel/a.bin")).unwrap(), a);
    assert!(t.out().join("Rel/Empty").is_dir());
    for d in ["Rel", "Rel/Empty"] {
        let mtime = std::fs::metadata(t.out().join(d))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(mtime, 1_700_000_000, "{d}");
    }
}

// ---- Persistent extraction throughput (run by hand) ----
//
// `RG_BENCH_DIR=<dir> RG_BENCH_MIB=2048 RG_BENCH_VOL_MIB=100 cargo test --release --lib
// persistent_throughput -- --ignored --nocapture --test-threads=1`. Generates a stored RAR5
// set and an AES-256 stored RAR5 set (plain headers) once under the directory, then extracts
// each through the persistent path with a live (not final) feed, once per tuning, and prints
// MB/s and durable syncs. `RG_BENCH_TUNINGS` picks tunings by name (default: all).

/// A live download whose volumes are all on disk already.
struct LiveComplete(u64);
impl VolumeFeed for LiveComplete {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else {
            VolumeView::NotYet
        }
    }
    fn expected_volumes(&self) -> u64 {
        self.0
    }
}

/// A live download whose volumes are on disk, handed over with lost article ranges that the
/// repair has already put back: the session skips them and patches them inline.
struct LiveDamaged(u64, Vec<(PathBuf, Vec<(u64, u64)>)>);
impl VolumeFeed for LiveDamaged {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if !path.is_file() {
            return VolumeView::NotYet;
        }
        let holes = self
            .1
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, h)| h.clone())
            .unwrap_or_default();
        VolumeView::Ready { holes }
    }
    fn expected_volumes(&self) -> u64 {
        self.0
    }
    fn wait_repaired(&mut self, _is_cancelled: &mut dyn FnMut() -> bool) -> bool {
        true
    }
    fn repaired(&self, _path: &Path) -> bool {
        true
    }
}

/// Lost ranges of a stored set: per volume, a run of 12 adjacent 768,000-byte articles 1 MiB
/// into the payload and three single articles after it.
fn bench_holes(first: &Path, count: usize, vol: u64) -> Vec<(PathBuf, Vec<(u64, u64)>)> {
    const ARTICLE: u64 = 768_000;
    let dir = first.parent().unwrap();
    (0..count)
        .map(|i| {
            let path = dir.join(volume_name(i, count));
            let len = std::fs::metadata(&path).unwrap().len();
            let part = if i + 1 == count { len } else { vol };
            let payload = len - end_block(false).len() as u64 - part.min(len);
            let mut holes = vec![(payload + (1 << 20), 12 * ARTICLE)];
            for k in 0..3u64 {
                let at = payload + (1 << 20) + (20 + 25 * k) * ARTICLE;
                if at + ARTICLE + (1 << 20) < len {
                    holes.push((at, ARTICLE));
                }
            }
            holes.retain(|&(s, l)| s + l + (1 << 20) < len);
            (path, holes)
        })
        .collect()
}

fn bench_fill(buf: &mut [u8], seed: &mut u64) {
    for c in buf.chunks_mut(8) {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        let b = seed.to_le_bytes();
        c.copy_from_slice(&b[..c.len()]);
    }
}

/// Writes the set once (a `complete` marker makes later runs reuse it). Returns the first
/// volume and the volume count.
fn bench_set(dir: &Path, total: u64, vol: u64, password: Option<&str>) -> (PathBuf, usize) {
    let count = total.div_ceil(vol) as usize;
    assert!(count < 100, "volume names hold two digits");
    let first = dir.join(volume_name(0, count));
    if dir.join("complete").exists() {
        return (first, count);
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let key = password.map(|pw| {
        zipfast::crypto::derive_rar5_key_and_hash_key(pw, 1, &RAR5_SALT, None)
            .unwrap()
            .0
    });
    // Ciphertext is the padded plaintext's length: stored parts cut it, not the plaintext.
    let stream = if key.is_some() {
        total.next_multiple_of(16)
    } else {
        total
    };
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut whole = crc32fast::Hasher::new();
    let mut iv = RAR5_IV;
    let mut done = 0u64;
    for i in 0..count {
        let last = i + 1 == count;
        let len = if last { stream - done } else { vol };
        let plain_len = len.min(total.saturating_sub(done));
        let mut part = vec![0u8; len as usize];
        bench_fill(&mut part[..plain_len as usize], &mut seed);
        whole.update(&part[..plain_len as usize]);
        if let Some(key) = &key {
            part = zipfast::crypto::encrypt_cbc(key, &iv, &part).unwrap();
            iv.copy_from_slice(&part[part.len() - 16..]);
        }
        let crc = if last {
            whole.clone().finalize()
        } else {
            crc32fast::hash(&part)
        };
        let mut out = SIG.to_vec();
        out.extend(main_block(count > 1, (i > 0).then_some(i as u64)));
        if key.is_some() {
            out.extend(rar5_enc_file_header(
                "movie.mkv",
                part.len() as u64,
                total,
                crc,
                i > 0,
                !last,
            ));
            out.extend_from_slice(&part);
        } else {
            out.extend(file_block("movie.mkv", total, &part, crc, i > 0, !last));
        }
        out.extend(end_block(!last));
        std::fs::write(dir.join(volume_name(i, count)), out).unwrap();
        done += len;
    }
    std::fs::write(dir.join("complete"), b"").unwrap();
    (first, count)
}

#[test]
#[ignore = "benchmark: run by hand"]
fn persistent_throughput() {
    let env = |k: &str, d: u64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let root = std::env::var("RG_BENCH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("rg-persistent-bench"));
    let total = env("RG_BENCH_MIB", 2048) << 20;
    let vol = env("RG_BENCH_VOL_MIB", 100) << 20;
    let rounds = env("RG_BENCH_ROUNDS", 1);
    let pick = std::env::var("RG_BENCH_TUNINGS").unwrap_or_default();
    let sets = std::env::var("RG_BENCH_SETS").unwrap_or_default();
    let tunings: &[(&str, (u64, Duration, u32))] = &[
        // Everything as before this tuning (see `persistent::LEGACY`).
        ("zipfast-default", (64 << 20, Duration::from_secs(2), 1 << 20)),
        ("group64", (64 << 20, Duration::from_secs(30), 8 << 20)),
        ("group256", (256 << 20, Duration::from_secs(30), 8 << 20)),
        // group1g without `group_refills` (see `persistent::GROUP_REFILLS`).
        ("group1g-norefills", (1 << 30, Duration::from_secs(30), 8 << 20)),
        ("group1g", (1 << 30, Duration::from_secs(30), 8 << 20)),
        ("group4g", (4 << 30, Duration::from_secs(60), 8 << 20)),
    ];
    for (set, password) in [("stored", None), ("aes", Some("pw")), ("holes", None)] {
        if !sets.is_empty() && !sets.split(',').any(|s| s == set) {
            continue;
        }
        let damaged = set == "holes";
        let kind = if damaged { "stored" } else { set };
        let dir = root.join(format!("{kind}-{}-{}", total >> 20, vol >> 20));
        let (first, count) = bench_set(&dir, total, vol, password);
        for round in 0..rounds {
            for (name, tuning) in tunings {
                if !pick.is_empty() && !pick.split(',').any(|p| p == *name) {
                    continue;
                }
                let out = root.join("out");
                let _ = std::fs::remove_dir_all(&out);
                std::fs::create_dir_all(&out).unwrap();
                persistent::TUNING.with(|t| t.set(Some(*tuning)));
                persistent::LEGACY.with(|c| c.set(*name == "zipfast-default"));
                persistent::GROUP_REFILLS.with(|c| c.set(*name != "group1g-norefills"));
                let syncs = zipfast::partial::durable_sync_count();
                let anchor = anchors::anchor_sync_count();
                let t0 = std::time::Instant::now();
                let feed: Box<dyn VolumeFeed> = if damaged {
                    Box::new(LiveDamaged(count as u64, bench_holes(&first, count, vol)))
                } else {
                    Box::new(LiveComplete(count as u64))
                };
                let (r, log) = run_pw(&first, &out, password.unwrap_or(""), feed);
                let secs = t0.elapsed().as_secs_f64();
                persistent::TUNING.with(|t| t.set(None));
                persistent::LEGACY.with(|c| c.set(false));
                persistent::GROUP_REFILLS.with(|c| c.set(true));
                expect_ok(r, &log);
                assert!(
                    log.iter().any(|l| l.contains("into persistent partial files")),
                    "{log:#?}"
                );
                assert!(
                    !log.iter().any(|l| l.contains("progressive direct unpack")),
                    "{log:#?}"
                );
                assert_eq!(
                    std::fs::metadata(out.join("movie.mkv")).unwrap().len(),
                    total
                );
                let z = zipfast::partial::durable_sync_count() - syncs;
                let a = anchors::anchor_sync_count() - anchor;
                println!(
                    "BENCH set={set} round={round} tuning={name} bytes={total} secs={secs:.2} mbps={:.1} syncs={} zipfast_syncs={z} anchor_syncs={a} mib_per_sync={:.1}",
                    total as f64 / 1e6 / secs,
                    z + a,
                    (total >> 20) as f64 / (z + a).max(1) as f64
                );
                if damaged {
                    assert_eq!(
                        std::fs::read(out.join("movie.mkv")).unwrap().len() as u64,
                        total
                    );
                    println!(
                        "  holes reported {}, patch lines {}",
                        log.iter().filter(|l| l.contains("has a hole at")).count(),
                        log.iter().filter(|l| l.contains("while extracting (")).count()
                    );
                }
                if let Some(l) = log.iter().find(|l| l.contains("durable syncs")) {
                    println!("  log: {l}");
                }
                let _ = std::fs::remove_dir_all(&out);
            }
        }
    }
}

/// Copies a directory tree (files and folders only).
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let ty = e.file_type().unwrap();
        if ty.is_dir() {
            copy_tree(&e.path(), &to.join(e.file_name()));
        } else if ty.is_file() {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// Puts the tree `live` back to the copy `saved` the way a crash leaves it: a file in both keeps
/// its inode (zipfast holds outputs by identity) and gets the saved bytes and length, folders
/// stay, and what is only in `live` goes.
fn restore_tree(saved: &Path, live: &Path) {
    use std::io::Write as _;
    std::fs::create_dir_all(live).unwrap();
    for e in std::fs::read_dir(live).unwrap() {
        let e = e.unwrap();
        if !saved.join(e.file_name()).exists() {
            if e.file_type().unwrap().is_dir() {
                std::fs::remove_dir_all(e.path()).unwrap();
            } else {
                std::fs::remove_file(e.path()).unwrap();
            }
        }
    }
    for e in std::fs::read_dir(saved).unwrap() {
        let e = e.unwrap();
        let to = live.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            restore_tree(&e.path(), &to);
        } else if to.is_file() {
            let bytes = std::fs::read(e.path()).unwrap();
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&to)
                .unwrap();
            f.write_all(&bytes).unwrap();
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

/// A download whose damaged volume is handed over with its hole; once the session asks for
/// volume `snap_at` (the hole's group is still open: holes and the output around them join the
/// group commit), the set's whole persistent state is copied aside, which is what a crash at
/// that instant would leave on disk.
struct SnapshotAt {
    inner: Downloading,
    snap_at: PathBuf,
    root: PathBuf,
    saved: PathBuf,
    taken: Arc<AtomicBool>,
}

impl VolumeFeed for SnapshotAt {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path == self.snap_at && !self.taken.load(Ordering::Relaxed) {
            copy_tree(&self.root, &self.saved);
            self.taken.store(true, Ordering::Relaxed);
        }
        self.inner.volume(path)
    }
    fn wait_repaired(&mut self, c: &mut dyn FnMut() -> bool) -> bool {
        self.inner.wait_repaired(c)
    }
    fn expected_volumes(&self) -> u64 {
        self.inner.expected_volumes()
    }
}

#[test]
fn a_crash_before_a_grouped_hole_commits_reports_the_hole_again() {
    // First run: a hole in volume 2 joins the open group (`group_holes`); the state on disk at
    // the moment volume 4 is asked for is kept, and the run is cancelled. Restoring that state
    // stands in for a daemon killed before the group committed: the hole was never durable.
    // Second run: resumes from the committed output, meets the hole again, reports it and
    // patches it from the repaired volume.
    let t = TestDir::new("persist_crash_grouped_hole");
    let data = pattern(3_000_000);
    let chunk = 500_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let v2 = t.dl().join(volume_name(1, vols.len()));
    let p = payload_offset(&vols[1], chunk) as u64 + 100_000;
    let mut dmg = vols[1].clone();
    dmg[p as usize..p as usize + 50_000].fill(0);
    std::fs::write(&v2, &dmg).unwrap();
    let state = t.out().join(persistent::STATE_DIR);
    let saved = t.0.join("crash-state");
    let taken = Arc::new(AtomicBool::new(false));
    let feed = SnapshotAt {
        inner: Downloading {
            damaged: Damaged {
                holes: vec![(v2.clone(), vec![(p, 50_000)])],
                repair: Vec::new(),
                handed: false,
            },
            volumes: vols.len() as u64,
        },
        snap_at: t.dl().join(volume_name(3, vols.len())),
        root: state.clone(),
        saved: saved.clone(),
        taken: taken.clone(),
    };
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let mut log1 = Vec::new();
    let flag = taken.clone();
    let r1 = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log1.push(m.to_string()),
        &mut || flag.load(Ordering::Relaxed),
    );
    assert!(
        matches!(r1, Err(Error::Failed(Failure::Canceled))),
        "{log1:?}"
    );
    assert!(
        log1.iter().any(|l| l.contains("has a hole at")),
        "{log1:?}"
    );
    assert!(taken.load(Ordering::Relaxed), "{log1:?}");
    // The crash: what was on disk when volume 4 was asked for, not what the cancel committed.
    restore_tree(&saved, &state);
    // Volume 2 is still damaged on disk and handed over with its hole; its repair puts the
    // original bytes back.
    let damaged = Damaged {
        holes: vec![(v2.clone(), vec![(p, 50_000)])],
        repair: vec![(v2, vols[1].clone())],
        handed: false,
    };
    let (r2, log2) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r2, &log2);
    assert!(persistent(&log2), "{log2:?}");
    assert!(
        log2.iter().any(|l| l.contains("resuming movie.mkv")),
        "{log2:?}"
    );
    // Lost with the group, so met and reported again.
    assert!(
        log2.iter().any(|l| l.contains("has a hole at")),
        "{log2:?}"
    );
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(!state.exists());
}

/// `Downloading`, whose volumes count as repaired once volume `release` was asked for (after
/// the first member finished), and which answers volume `snap_at` only once `patched` is set:
/// then it copies the set's persistent state aside (as `SnapshotAt` does) and hands it over.
struct RefillThenSnapshot {
    inner: Downloading,
    release: PathBuf,
    released: AtomicBool,
    snap_at: PathBuf,
    patched: Arc<AtomicBool>,
    root: PathBuf,
    saved: PathBuf,
    taken: Arc<AtomicBool>,
}

impl VolumeFeed for RefillThenSnapshot {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path == self.release {
            self.released.store(true, Ordering::Relaxed);
        }
        if path == self.snap_at && !self.taken.load(Ordering::Relaxed) {
            if !self.patched.load(Ordering::Relaxed) {
                return VolumeView::NotYet;
            }
            copy_tree(&self.root, &self.saved);
            self.taken.store(true, Ordering::Relaxed);
        }
        self.inner.volume(path)
    }
    fn wait_repaired(&mut self, c: &mut dyn FnMut() -> bool) -> bool {
        self.inner.wait_repaired(c)
    }
    fn expected_volumes(&self) -> u64 {
        self.inner.expected_volumes()
    }
    fn repaired(&self, _path: &Path) -> bool {
        self.released.load(Ordering::Relaxed)
    }
}

#[test]
fn a_crash_before_a_grouped_refill_commits_refills_the_hole_again() {
    // a.bin (whole in volume 1) has a hole; it finishes, which commits the hole, and the hole
    // is refilled inline into a.bin's open group (`group_refills`) while b.bin goes on. The
    // state on disk when volume 3 is asked for is kept and the run cancelled: a crash before
    // that group committed, so the refilled bytes are a hole again. The second run adopts
    // a.bin as finished with its hole and refills it from the volumes.
    let t = TestDir::new("persist_crash_grouped_refill");
    let a = pattern(400_000);
    let b: Vec<u8> = pattern(900_000).iter().map(|x| x ^ 0x5a).collect();
    let b_parts: Vec<&[u8]> = b.chunks(300_000).collect();
    let mut vols = Vec::new();
    let mut a_payload = 0u64;
    for (i, part) in b_parts.iter().enumerate() {
        let last = i + 1 == b_parts.len();
        let mut v = SIG.to_vec();
        v.extend(main_block(true, (i > 0).then_some(i as u64)));
        if i == 0 {
            let block = file_block("a.bin", a.len() as u64, &a, crc32fast::hash(&a), false, false);
            a_payload = (v.len() + block.len() - a.len()) as u64;
            v.extend(block);
        }
        let crc = if last {
            crc32fast::hash(&b)
        } else {
            crc32fast::hash(part)
        };
        v.extend(file_block("b.bin", b.len() as u64, part, crc, i > 0, !last));
        v.extend(end_block(!last));
        vols.push(v);
    }
    let first = write_set(&t.dl(), &vols);
    let v1 = first.clone();
    let hole = (a_payload + 100_000, 50_000);
    let state = t.out().join(persistent::STATE_DIR);
    let saved = t.0.join("crash-state");
    let taken = Arc::new(AtomicBool::new(false));
    // The hole is handed over; the bytes on disk are intact, standing in for the repair.
    let patched = Arc::new(AtomicBool::new(false));
    let feed = RefillThenSnapshot {
        inner: Downloading {
            damaged: Damaged {
                holes: vec![(v1.clone(), vec![hole])],
                repair: Vec::new(),
                handed: false,
            },
            volumes: vols.len() as u64,
        },
        release: t.dl().join(volume_name(1, vols.len())),
        released: AtomicBool::new(false),
        snap_at: t.dl().join(volume_name(2, vols.len())),
        patched: patched.clone(),
        root: state.clone(),
        saved: saved.clone(),
        taken: taken.clone(),
    };
    let req = Request {
        first_volume: &first,
        dest: &t.out(),
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let mut log1 = Vec::new();
    let flag = taken.clone();
    let r1 = extract(
        &req,
        Box::new(feed),
        &mut |_, m: &str| {
            if m.starts_with("Unrar: patched a.bin from repaired") {
                patched.store(true, Ordering::Relaxed);
            }
            log1.push(m.to_string())
        },
        &mut || flag.load(Ordering::Relaxed),
    );
    assert!(
        matches!(r1, Err(Error::Failed(Failure::Canceled))),
        "{log1:?}"
    );
    assert!(
        log1.iter()
            .any(|l| l.starts_with("Unrar: patched a.bin from repaired")),
        "{log1:?}"
    );
    restore_tree(&saved, &state);
    let damaged = Damaged {
        holes: Vec::new(),
        repair: Vec::new(),
        handed: false,
    };
    let (r2, log2) = run_downloading(&first, &t.out(), damaged, vols.len() as u64);
    expect_ok(r2, &log2);
    assert!(persistent(&log2), "{log2:?}");
    assert!(
        log2.iter()
            .any(|l| l.contains("resuming a.bin") && l.contains("finished")),
        "{log2:?}"
    );
    // The refill was lost with its group: a.bin is refilled again.
    assert!(
        log2.iter()
            .any(|l| l == "Unrar: repaired a.bin from the repaired volumes (holes refilled)"),
        "{log2:?}"
    );
    assert_eq!(std::fs::read(t.out().join("a.bin")).unwrap(), a);
    assert_eq!(std::fs::read(t.out().join("b.bin")).unwrap(), b);
    assert!(!state.exists());
}

// ---- Writer-fed volumes (see `crate::livewrite`) ----

/// A download writing its volumes with direct write: every volume but the first is written
/// article by article into an output created at its length, each article reported to
/// `livewrite` once written, and renamed into place once complete. A volume in `holes` was
/// finished with those articles lost, and is "repaired" (`wait_repaired` writes its bytes).
struct WrittenFeed {
    nzb_id: i32,
    done: Arc<AtomicBool>,
    holes: Vec<(PathBuf, Vec<(u64, u64)>)>,
    repair: Vec<(PathBuf, Vec<u8>)>,
    repaired: bool,
    /// Every volume handed out as being written.
    handed: Arc<Mutex<Vec<Arc<crate::livewrite::LiveVolume>>>>,
}

impl WrittenFeed {
    /// Bytes read from the volumes as they were written (this feed's only).
    fn read_live(handed: &Mutex<Vec<Arc<crate::livewrite::LiveVolume>>>) -> u64 {
        handed.lock().unwrap().iter().map(|v| v.read_live()).sum()
    }
}

impl VolumeFeed for WrittenFeed {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path.is_file() {
            let holes = match self.holes.iter().find(|(p, _)| p == path) {
                Some((_, h)) if !self.repaired => h.clone(),
                _ => Vec::new(),
            };
            VolumeView::Ready { holes }
        } else if self.done.load(Ordering::Acquire) {
            VolumeView::Missing
        } else {
            VolumeView::NotYet
        }
    }
    fn growing(&mut self, path: &Path) -> Option<Arc<crate::livewrite::LiveVolume>> {
        if path.is_file() {
            return None;
        }
        let live = crate::livewrite::lookup(self.nzb_id, crate::fsops::path_to_bytes(path))?;
        self.handed.lock().unwrap().push(live.clone());
        Some(live)
    }
    fn wait_repaired(&mut self, _is_cancelled: &mut dyn FnMut() -> bool) -> bool {
        for (p, bytes) in &self.repair {
            std::fs::write(p, bytes).unwrap();
        }
        self.repaired = true;
        true
    }
    fn still_held(&self, path: &Path) -> bool {
        !self.repaired && self.holes.iter().any(|(p, _)| p == path)
    }
}

/// Starts writing volumes 1.. of `vols` into `dl` as a download does (see [`WrittenFeed`]),
/// `article` bytes at a time, each half of a volume in reverse order; `lose` (volume, offset,
/// length) is never written. Returns the writer.
fn write_like_a_download(
    dl: &Path,
    vols: &[Vec<u8>],
    nzb_id: i32,
    article: usize,
    lose: Option<(usize, u64, u64)>,
    done: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let dl = dl.to_path_buf();
    let vols = vols.to_vec();
    std::thread::spawn(move || {
        use std::os::unix::fs::FileExt;
        let n = vols.len();
        for (i, v) in vols.iter().enumerate().skip(1) {
            let out = dl.join(format!("{i}.out.tmp"));
            let fin = dl.join(volume_name(i, n));
            let f = std::fs::File::create(&out).unwrap();
            f.set_len(v.len() as u64).unwrap();
            crate::livewrite::register(
                nzb_id,
                i as i32,
                crate::fsops::path_to_bytes(&fin),
                crate::fsops::path_to_bytes(&out),
                v.len() as u64,
                true,
            );
            let count = v.len().div_ceil(article);
            let half = count / 2;
            let order = (0..half).rev().chain((half..count).rev());
            for a in order {
                let start = a * article;
                let end = (start + article).min(v.len());
                let lost = lose
                    .is_some_and(|(lv, lo, ll)| lv == i && (start as u64) < lo + ll && lo < end as u64);
                if !lost {
                    f.write_all_at(&v[start..end], start as u64).unwrap();
                    crate::livewrite::written(nzb_id, i as i32, start as u64, (end - start) as u64);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            drop(f);
            std::fs::rename(&out, &fin).unwrap();
        }
        crate::livewrite::forget(nzb_id);
        done.store(true, Ordering::Release);
    })
}

type WrittenSet = (Vec<u8>, PathBuf, WrittenFeed, std::thread::JoinHandle<()>);

fn written_set(t: &TestDir, nzb_id: i32, lose: bool) -> WrittenSet {
    let data = pattern(1_200_000);
    let chunk = 100_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let n = vols.len();
    let first = t.dl().join(volume_name(0, n));
    std::fs::write(&first, &vols[0]).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let lost = lose.then(|| (5, payload_offset(&vols[5], chunk) as u64 + 30_000, 10_000));
    let mut feed = WrittenFeed {
        nzb_id,
        done: done.clone(),
        holes: Vec::new(),
        repair: Vec::new(),
        repaired: false,
        handed: Arc::new(Mutex::new(Vec::new())),
    };
    if let Some((v, lo, ll)) = lost {
        let p = t.dl().join(volume_name(v, n));
        // The writer's articles are 10 000 bytes from offset 0: the hole is the articles the
        // lost range touches.
        let start = lo / 10_000 * 10_000;
        let end = ((lo + ll).div_ceil(10_000) * 10_000).min(vols[v].len() as u64);
        feed.holes.push((p.clone(), vec![(start, end - start)]));
        feed.repair.push((p, vols[v].clone()));
    }
    let writer = write_like_a_download(&t.dl(), &vols, nzb_id, 10_000, lost, done);
    (data, first, feed, writer)
}

#[test]
fn volumes_being_written_are_read_article_by_article() {
    let t = TestDir::new("written");
    let (data, first, feed, writer) = written_set(&t, -7_320_001, false);
    let handed = feed.handed.clone();
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    writer.join().unwrap();
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(WrittenFeed::read_live(&handed) > 0, "nothing was read as written");
}

#[test]
fn volumes_being_written_are_read_article_by_article_on_the_copy_path() {
    let t = TestDir::new("written-copy");
    let (data, first, feed, writer) = written_set(&t, -7_320_002, false);
    let handed = feed.handed.clone();
    let mut log = Vec::new();
    let mut outputs = Vec::new();
    let out = t.out();
    let req = Request {
        first_volume: &first,
        dest: &out,
        password: "",
        overwrite: Overwrite::Always,
        on_wait: None,
    };
    let r = extract_recording(
        &req,
        Box::new(feed),
        &mut |_, m: &str| log.push(m.to_string()),
        &mut || false,
        Some(&mut outputs),
    );
    writer.join().unwrap();
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(WrittenFeed::read_live(&handed) > 0, "nothing was read as written");
}

#[test]
fn a_lost_article_in_a_volume_being_written_is_a_hole_patched_after_the_repair() {
    let t = TestDir::new("written-hole");
    let (data, first, feed, writer) = written_set(&t, -7_320_003, true);
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    writer.join().unwrap();
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(log.iter().any(|l| l.contains("extracted past damage")), "{log:?}");
}

#[test]
fn a_volume_the_download_never_finished_is_lost_as_damage() {
    let t = TestDir::new("written-gone");
    let data = pattern(600_000);
    let chunk = 100_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let n = vols.len();
    let first = t.dl().join(volume_name(0, n));
    std::fs::write(&first, &vols[0]).unwrap();
    std::fs::write(t.dl().join(volume_name(1, n)), &vols[1]).unwrap();
    let nzb_id = -7_320_004;
    let done = Arc::new(AtomicBool::new(false));
    let feed = WrittenFeed {
        nzb_id,
        done: done.clone(),
        holes: Vec::new(),
        repair: Vec::new(),
        repaired: false,
        handed: Arc::new(Mutex::new(Vec::new())),
    };
    let handed = feed.handed.clone();
    // Volume 3 is written halfway, then the download ends without it.
    let dl = t.dl();
    let v = vols[2].clone();
    let writer = std::thread::spawn(move || {
        use std::os::unix::fs::FileExt;
        let out = dl.join("2.out.tmp");
        let f = std::fs::File::create(&out).unwrap();
        f.set_len(v.len() as u64).unwrap();
        crate::livewrite::register(
            nzb_id,
            2,
            crate::fsops::path_to_bytes(&dl.join(volume_name(2, n))),
            crate::fsops::path_to_bytes(&out),
            v.len() as u64,
            true,
        );
        let half = v.len() / 2;
        f.write_all_at(&v[..half], 0).unwrap();
        crate::livewrite::written(nzb_id, 2, 0, half as u64);
        std::thread::sleep(Duration::from_millis(300));
        crate::livewrite::forget(nzb_id);
        done.store(true, Ordering::Release);
    });
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    writer.join().unwrap();
    match r {
        Err(Error::Failed(f)) => assert!(f.is_damage(), "{f:?} {log:?}"),
        other => panic!("expected a damage failure, got ok={}", other.is_ok()),
    }
    assert!(WrittenFeed::read_live(&handed) > 0, "nothing was read as written");
    assert!(
        handed.lock().unwrap().iter().all(|v| !v.is_waited()),
        "a reader still waits after the end"
    );
}

/// A volume the download starts writing, then dies in before any byte of it was read, rebuilt
/// from a duplicate (`deadvolume`): the extraction parked on it reads the stand-in from its
/// start. Once bytes were read from what the download wrote, a stand-in cannot take over.
struct DyingFeed {
    nzb_id: i32,
    dead: PathBuf,
    stand_in: Arc<Mutex<Option<crate::deadvolume::VirtualVolume>>>,
    volumes: u64,
}

impl VolumeFeed for DyingFeed {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path == self.dead {
            return match self.stand_in.lock().unwrap().clone() {
                Some(v) => VolumeView::Virtual(v),
                None => VolumeView::NotYet,
            };
        }
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else {
            VolumeView::Missing
        }
    }
    fn growing(&mut self, path: &Path) -> Option<Arc<crate::livewrite::LiveVolume>> {
        if path != self.dead || self.stand_in.lock().unwrap().is_some() {
            return None;
        }
        crate::livewrite::lookup(self.nzb_id, crate::fsops::path_to_bytes(path))
    }
    fn expected_volumes(&self) -> u64 {
        self.volumes
    }
}

fn dying_volume_run(tag: &str, nzb_id: i32, write_header: bool, other_layout: bool) -> (
    Result<zipfast::extract::Summary, Error>,
    Vec<String>,
    Vec<u8>,
    TestDir,
) {
    dying_volume_run_on(tag, nzb_id, write_header, other_layout, true)
}

/// `persistent`: the feed gives a volume count (a persistent session); without one the copy
/// path extracts.
fn dying_volume_run_on(
    tag: &str,
    nzb_id: i32,
    write_header: bool,
    other_layout: bool,
    persistent: bool,
) -> (Result<zipfast::extract::Summary, Error>, Vec<String>, Vec<u8>, TestDir) {
    let t = TestDir::new(tag);
    let data = pattern(400_000);
    let chunk = 100_000;
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let n = vols.len();
    for i in [0, 1, 3] {
        std::fs::write(t.dl().join(volume_name(i, n)), &vols[i]).unwrap();
    }
    let dead = t.dl().join(volume_name(2, n));
    let out = t.dl().join("2.out.tmp");
    let f = std::fs::File::create(&out).unwrap();
    f.set_len(vols[2].len() as u64).unwrap();
    crate::livewrite::register(
        nzb_id,
        2,
        crate::fsops::path_to_bytes(&dead),
        crate::fsops::path_to_bytes(&out),
        vols[2].len() as u64,
        true,
    );
    if write_header {
        use std::os::unix::fs::FileExt;
        f.write_all_at(&vols[2][..10_000], 0).unwrap();
        crate::livewrite::written(nzb_id, 2, 0, 10_000);
    }
    // The stand-in: the volume's member data in a scratch file between its headers and its
    // end. An alt's is laid out otherwise: other (here shorter) headers, the same member data
    // placed by the content map, its own end.
    let whole = t.dl().join("whole-2.bin");
    std::fs::write(&whole, &vols[2]).unwrap();
    let (data_at, packed) = super::first_member_data(&whole).unwrap();
    let (data_at, packed) = (data_at as usize, packed as usize);
    let mid = t.dl().join("borrowed.bin");
    std::fs::write(&mid, &vols[2][data_at..data_at + packed]).unwrap();
    let head_bytes = if other_layout {
        vols[2][..data_at - 4].to_vec()
    } else {
        vols[2][..data_at].to_vec()
    };
    let v = crate::deadvolume::VirtualVolume {
        head: head_bytes,
        data: mid,
        data_len: packed as u64,
        tail: vols[2][data_at + packed..].to_vec(),
        holes: Vec::new(),
    };
    let stand_in = Arc::new(Mutex::new(None));
    let set = stand_in.clone();
    let live = crate::livewrite::lookup(nzb_id, crate::fsops::path_to_bytes(&dead)).unwrap();
    let rebuilt = std::thread::spawn(move || {
        // The download dies once the extraction read what it wrote (when it wrote anything).
        let t0 = std::time::Instant::now();
        while write_header && live.read_live() == 0 && t0.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(100));
        *set.lock().unwrap() = Some(v);
    });
    let feed = DyingFeed { nzb_id, dead, stand_in, volumes: if persistent { n as u64 } else { 0 } };
    let first = t.dl().join(volume_name(0, n));
    let (r, log) = run(&first, &t.out(), Box::new(feed));
    rebuilt.join().unwrap();
    crate::livewrite::forget(nzb_id);
    (r, log, data, t)
}

#[test]
fn a_volume_rebuilt_before_any_byte_was_read_is_read_from_the_stand_in() {
    let (r, log, data, t) = dying_volume_run("written-rebuilt", -7_320_005, false, false);
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn a_twin_stand_in_takes_over_a_volume_after_bytes_were_read() {
    let (r, log, data, t) = dying_volume_run("written-rebuilt-twin", -7_320_006, true, false);
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}

#[test]
fn an_alt_stand_in_continues_the_volume_at_its_payload_frontier_without_a_restart() {
    let (r, log, data, t) = dying_volume_run("written-rebuilt-alt", -7_320_007, true, true);
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
    assert!(
        !log.iter().any(|l| l.contains("progressive direct unpack")),
        "the extraction started over: {log:?}"
    );
}

#[test]
fn an_alt_stand_in_continues_the_volume_on_the_copy_path_too() {
    let (r, log, data, t) = dying_volume_run_on("written-rebuilt-alt-copy", -7_320_008, true, true, false);
    expect_ok(r, &log);
    assert_eq!(std::fs::read(t.out().join("movie.mkv")).unwrap(), data);
}
