use super::*;
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
        overwrite: Overwrite::Always,
        on_wait: None,
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
            return if path.is_file() { VolumeView::Ready { holes: Vec::new() } } else { VolumeView::Missing };
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
        inner: Damaged { holes, repair, handed: false },
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
    assert!(log.iter().any(|l| l.contains("repaired movie.mkv from the repaired volumes")), "{log:?}");
    assert!(copied.is_empty(), "copied into place instead of moved: {copied:?}");
}

/// Without the final-set report the same repaired output is still copied into place.
#[test]
fn repaired_output_of_an_unfinished_set_is_copied() {
    let t = TestDir::new("unfinishedrepair");
    let chunk = 50_000;
    let data = pattern(6 * chunk);
    let vols = rar5_volumes("movie.mkv", &data, chunk);
    let first = write_set(&t.dl(), &vols);
    let path = t.dl().join(volume_name(2, vols.len()));
    let p = payload_offset(&vols[2], chunk) as u64;
    let mut dmg = vols[2].clone();
    dmg[(p + 10_000) as usize..(p + 18_000) as usize].fill(0);
    std::fs::write(&path, &dmg).unwrap();
    let feed = Damaged {
        holes: vec![(path.clone(), vec![(p + 10_000, 8_000)])],
        repair: vec![(path, vols[2].clone())],
        handed: false,
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
    assert_eq!(copied.len(), 1, "{copied:?}");
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
fn rar4_solid_compressed_is_reported_unsupported() {
    // A RAR4 (1.5-4.x) archive with a solid compressed member (method 0x33): zipfast refuses
    // solid RAR4 with a typed error, so the fallback decoder takes it.
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

fn rar4_store_volumes(name: &str, data: &[u8], chunk: usize) -> Vec<Vec<u8>> {
    let parts: Vec<&[u8]> = data.chunks(chunk).collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let last = i + 1 == n;
            let crc = if last { crc32fast::hash(data) } else { crc32fast::hash(part) };
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
            let h = rar4_file_header(name, part.len(), data.len(), crc, i > 0, !last, Some(&RAR4_SALT));
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
    let r = extract(&req, feed, &mut |_, m: &str| log.push(m.to_string()), &mut || false);
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
    let first = write_named(&t.dl(), &rar4_store_volumes("movie.mkv", &data, 60_000), rar4_name);
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
            let info = crate::postrepair::parse_unpack_message(l.as_bytes(), b"", false);
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

fn rar5_enc_file_header(name: &str, packed: u64, size: u64, crc: u32, before: bool, after: bool) -> Vec<u8> {
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
    vint(3 | if before { 8 } else { 0 } | if after { 16 } else { 0 }, &mut body);
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
fn rar5_hp_volumes(name: &str, data: &[u8], chunk: usize, password: &str) -> (Vec<Vec<u8>>, Vec<usize>) {
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
        v.extend(rar5_wire(main_block(true, (i > 0).then_some(i as u64)), &key, 0));
        // Non-final parts carry the CRC of their packed (cipher) bytes, the last one the whole
        // plaintext CRC.
        let crc = if last {
            crc32fast::hash(data)
        } else {
            crc32fast::hash(part)
        };
        v.extend(rar5_wire(
            rar5_enc_file_header(name, part.len() as u64, data.len() as u64, crc, i > 0, !last),
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
        v.extend(file_block(name, data.len() as u64, data, crc32fast::hash(data), false, false));
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
    assert_eq!(meta.permissions().mode() & 0o111, 0o111, "executable bits restored");
    let mtime = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(mtime, 1_700_000_000);
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
    assert_eq!(std::fs::read(t.out().join("movie.mkv.rgpart")).unwrap(), b"keep me");
    let leftovers: Vec<_> = std::fs::read_dir(t.out())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".rustyget-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn rar4_stored_then_solid_compressed_reports_unsupported_for_the_fallback() {
    let t = TestDir::new("rar4mixed");
    let stored = pattern(20_000);
    let mut v = b"Rar!\x1a\x07\x00".to_vec();
    v.extend(rar4_block(0x73, 0, &[0u8; 6], 0));
    v.extend(rar4_file_header("a.bin", stored.len(), stored.len(), crc32fast::hash(&stored), false, false, None));
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
    // Solid compressed: refused by zipfast with a typed error.
    v.extend(rar4_block(0x74, 0x8000 | 0x0010, &rest, packed.len()));
    v.extend(&packed);
    v.extend(rar4_block(0x7b, 0, &[], 0));
    let first = t.dl().join("mixed.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    assert!(matches!(r, Err(Error::Unsupported(_))), "log: {log:?}");

    // Rename mode: the stored member already copied into place is taken back, so the fallback
    // decoder does not add a renamed second copy.
    let out2 = t.0.join("out2");
    let req = Request {
        first_volume: &first,
        dest: &out2,
        password: "",
        overwrite: Overwrite::Rename,
        on_wait: None,
    };
    let r = extract(&req, Box::new(CompleteSet), &mut |_, _| {}, &mut || false);
    assert!(matches!(r, Err(Error::Unsupported(_))));
    let left: Vec<_> = std::fs::read_dir(&out2)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
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
    assert_eq!(std::fs::read(t.out().join("Release/movie.mkv")).unwrap(), data);
}

#[test]
fn rename_mode_keeps_duplicate_members_and_existing_files() {
    let t = TestDir::new("rename");
    std::fs::write(t.out().join("a.bin"), b"existing").unwrap();
    let one = pattern(3_000);
    let two = pattern(4_000);
    let mut v = SIG.to_vec();
    v.extend(main_block(false, None));
    v.extend(file_block("a.bin", one.len() as u64, &one, crc32fast::hash(&one), false, false));
    v.extend(file_block("a.bin", two.len() as u64, &two, crc32fast::hash(&two), false, false));
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
    let r = extract(&req, Box::new(CompleteSet), &mut |_, m: &str| log.push(m.to_string()), &mut || false);
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
    v.extend(file_block("big.mkv", big.len() as u64, &big, crc32fast::hash(&big), false, false));
    let extra_at = v.len() + file_block("extra.bin", extra.len() as u64, &[], 0, false, false).len();
    v.extend(file_block("extra.bin", extra.len() as u64, &extra, crc32fast::hash(&extra), false, false));
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
    v.extend(file_block("a.bin", first_copy.len() as u64, &first_copy, crc32fast::hash(&first_copy), false, false));
    v.extend(file_block("a.bin", second_copy.len() as u64, &second_copy, crc32fast::hash(&second_copy), false, false));
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
    let mode = |n: &str| std::fs::metadata(t.out().join(n)).unwrap().permissions().mode();
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
    v.extend(file_block("a.bin", two.len() as u64, &two, crc32fast::hash(&two), false, false));
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
    v.extend(file_block("main.mkv", main.len() as u64, &main, crc32fast::hash(&main), false, false));
    let at = v.len() + file_block("doc.bin", one.len() as u64, &[], 0, false, false).len();
    v.extend(file_block("doc.bin", one.len() as u64, &one, crc32fast::hash(&one), false, false));
    v.extend(file_block("doc.bin", two.len() as u64, &two, crc32fast::hash(&two), false, false));
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
    let r = extract(&req, Box::new(feed), &mut |_, m: &str| log.push(m.to_string()), &mut || false);
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
    v.extend(file_block("main.mkv", main.len() as u64, &main, crc32fast::hash(&main), false, false));
    v.extend(file_block("doc.bin", a.len() as u64, &a, crc32fast::hash(&a), false, false));
    v.extend(file_block("doc_1.bin", b.len() as u64, &b, crc32fast::hash(&b), false, false));
    let at = v.len() + file_block("doc.bin", c.len() as u64, &[], 0, false, false).len();
    v.extend(file_block("doc.bin", c.len() as u64, &c, crc32fast::hash(&c), false, false));
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
    let r = extract(&req, Box::new(feed), &mut |_, m: &str| log.push(m.to_string()), &mut || false);
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
    let names: Vec<PathBuf> = (0..vols.len()).map(|i| t.dl().join(volume_name(i, vols.len()))).collect();
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
    v.extend(file_block("doc.bin", first_copy.len() as u64, &first_copy, crc32fast::hash(&first_copy), false, false));
    for i in 0..24u8 {
        let d: Vec<u8> = pattern(1_000).iter().map(|b| b ^ i).collect();
        v.extend(file_block("doc.bin", d.len() as u64, &d, crc32fast::hash(&d), false, false));
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
    v.extend(file_block("a.bin", two.len() as u64, &two, crc32fast::hash(&two), false, false));
    v.extend(file_block("a_1.bin", three.len() as u64, &three, crc32fast::hash(&three), false, false));
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
        v.extend(rar4_lz_member(&format!("m{}.bin", i + 1), packed, &gold, 0x31 + i as u8));
    }
    v.extend(rar4_block(0x7b, 0x4000, &[], 0));
    let first = t.dl().join("lz.rar");
    std::fs::write(&first, &v).unwrap();
    let (r, log) = run(&first, &t.out(), Box::new(CompleteSet));
    let s = expect_ok(r, &log);
    assert_eq!(s.files, 5);
    for i in 1..=5 {
        assert_eq!(std::fs::read(t.out().join(format!("m{i}.bin"))).unwrap(), gold, "m{i}");
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
        v.extend(rar4_block(0x73, 0x0001 | 0x0010 | if i == 0 { 0x0100 } else { 0 }, &[0u8; 6], 0));
        v.extend(rar4_lz_member(&format!("part{i}.bin"), LZ[2], &gold, 0x33));
        v.extend(rar4_block(0x7b, 0x4000 | if i + 1 < n { 0x0001 } else { 0 }, &[], 0));
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
        assert_eq!(std::fs::read(t.out().join(format!("part{i}.bin"))).unwrap(), gold);
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
                VolumeView::Ready { holes: vec![(self.1, 8)] }
            } else {
                VolumeView::Missing
            }
        }
    }
    let (r, log) = run(&first, &t.out(), Box::new(Hole(first.clone(), at as u64 + 300)));
    match r {
        Err(Error::Failed(f)) => assert!(f.is_damage(), "{f:?}"),
        _ => panic!("expected damage: {log:?}"),
    }
    assert!(!t.out().join("m.bin").exists());
}
