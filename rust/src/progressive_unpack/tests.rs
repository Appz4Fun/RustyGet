use super::*;
use std::sync::atomic::AtomicUsize;

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

fn file_block(name: &str, size: u64, part: &[u8], crc: u32, before: bool, after: bool) -> Vec<u8> {
    let mut fields = Vec::new();
    vint(0x4 | 0x2, &mut fields); // CRC present, mtime present
    vint(size, &mut fields);
    vint(0o644, &mut fields);
    fields.extend_from_slice(&1_700_000_000u32.to_le_bytes());
    fields.extend_from_slice(&crc.to_le_bytes());
    vint(0, &mut fields); // stored
    vint(1, &mut fields); // unix
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
            out.extend(main_block(multi, if i == 0 { None } else { Some(i as u64) }));
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
    let mut log = Vec::new();
    let req = Request {
        first_volume: first,
        dest: out,
        password: "",
        overwrite: true,
    };
    let r = extract(&req, feed, &mut |_, m: &str| log.push(m.to_string()), &mut || false);
    (r, log)
}

fn expect_ok(r: Result<zipfast::extract::Summary, Error>, log: &[String]) -> zipfast::extract::Summary {
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
    assert!(log.iter().any(|l| l == "Unrar: All OK"), "{log:?}");
    assert!(log.iter().any(|l| l == "Unrar: Extracting  movie.mkv"), "{log:?}");
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
        log.iter().filter(|l| l.starts_with("Unrar: Extracting from")).count(),
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
        other => panic!("expected a missing-volume failure, got ok={}", other.is_ok()),
    }
    assert!(!t.out().join("movie.mkv").exists());
}

/// Volumes become available one at a time, the session parks in between.
struct Trickle {
    released: Arc<AtomicUsize>,
    names: Vec<PathBuf>,
    waker: Arc<Mutex<Option<Waker>>>,
    asked: Arc<AtomicUsize>,
}

impl VolumeFeed for Trickle {
    fn volume(&mut self, path: &Path) -> VolumeView {
        self.asked.fetch_add(1, Ordering::Relaxed);
        let idx = self.names.iter().position(|n| n == path);
        match idx {
            Some(i) if i < self.released.load(Ordering::Acquire) => VolumeView::Ready { holes: Vec::new() },
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
    let names: Vec<PathBuf> = (0..vols.len()).map(|i| t.dl().join(volume_name(i, vols.len()))).collect();
    let released = Arc::new(AtomicUsize::new(1));
    let waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
    let feed = Trickle {
        released: released.clone(),
        names,
        waker: waker.clone(),
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
                VolumeView::Ready { holes: vec![(self.1, 1000)] }
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
fn rar4_compressed_is_reported_unsupported() {
    // A RAR4 (1.5-4.x) archive with a compressed member: method 0x33.
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
    fh.extend_from_slice(&0x8000u16.to_le_bytes());
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
    assert!(matches!(r, Err(Error::Unsupported(_))), "log: {log:?}");
}

#[test]
fn orphan_continuation_volume_is_refused() {
    // Ported from archive_native: a set that starts with a split-before member.
    let t = TestDir::new("orphan");
    let payload = b"tail-bytes";
    let mut v = SIG.to_vec();
    v.extend(main_block(true, None));
    v.extend(file_block("movie.bin", payload.len() as u64, payload, crc32fast::hash(payload), true, false));
    v.extend(end_block(false));
    let first = t.dl().join("orphan.part2.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    assert!(r.is_err(), "an orphan continuation must not extract: {log:?}");
    assert!(!t.out().join("movie.bin").exists());
}
