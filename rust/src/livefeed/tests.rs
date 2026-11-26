use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn names(list: &[&str]) -> Vec<Vec<u8>> {
    list.iter().map(|n| n.as_bytes().to_vec()).collect()
}

#[test]
fn sets_are_recognised_by_their_volume_names() {
    assert!(is_live_first(b"Movie.7z.001"));
    assert!(is_live_first(b"dir/movie.zip.001"));
    assert!(is_live_first(b"movie.z01"));
    assert!(is_live_first(b"movie.mkv.001"));
    assert!(!is_live_first(b"movie.7z.002"));
    assert!(
        !is_live_first(b"movie.rar.001"),
        "a byte-split RAR is the RAR path's"
    );
    assert!(!is_live_first(b"movie.part01.rar"));
    assert!(!is_live_first(b"movie.zip"));
    assert!(!is_live_first(b"movie.7z"));
    assert!(is_live_volume(b"movie.mkv.017"));
    assert!(is_live_volume(b"movie.z12"));
    assert!(!is_live_volume(b"movie.zip"));
    assert!(!is_live_volume(b"movie.mkv"));
    assert!(!is_live_volume(b"movie.vol01+02.par2"));
}

#[test]
fn a_set_is_every_volume_from_one_without_a_gap() {
    let all = names(&[
        "a.7z.002", "a.7z.001", "a.7z.003", "a.nfo", "b.7z.001", "A.7Z.004",
    ]);
    let set = live_set(b"a.7z.001", &all).unwrap();
    assert_eq!(set.kind, SetKind::SevenZip);
    assert_eq!(
        set.volumes,
        names(&["a.7z.001", "a.7z.002", "a.7z.003", "A.7Z.004"])
    );
    assert_eq!(set.last(), b"A.7Z.004");
    // a gap: not a set this download carries whole
    assert!(live_set(b"a.7z.001", &names(&["a.7z.001", "a.7z.003"])).is_none());
    // one volume is no split
    assert!(live_set(b"b.7z.001", &all).is_none());
    // Info-ZIP: the .zip comes last
    let z = names(&["m.zip", "m.z02", "m.z01"]);
    let set = live_set(b"m.z01", &z).unwrap();
    assert_eq!(set.kind, SetKind::ZipSplit);
    assert_eq!(set.volumes, names(&["m.z01", "m.z02", "m.zip"]));
    assert!(
        live_set(b"m.z01", &names(&["m.z01", "m.z02"])).is_none(),
        "no .zip yet"
    );
    let s = live_set(b"x.mkv.001", &names(&["x.mkv.002", "x.mkv.001"])).unwrap();
    assert_eq!(s.kind, SetKind::Split);
}

#[test]
fn post_processing_knows_the_volumes_of_an_extracted_set() {
    let firsts: Vec<&[u8]> = vec![b"Movie.7z.001", b"m.z01", b"x.mkv.001"];
    assert!(in_extracted_set(b"movie.7z.004", &firsts));
    assert!(in_extracted_set(b"m.zip", &firsts));
    assert!(in_extracted_set(b"m.z07", &firsts));
    assert!(in_extracted_set(b"x.mkv.009", &firsts));
    assert!(!in_extracted_set(b"other.7z.002", &firsts));
    assert!(!in_extracted_set(b"x.avi.002", &firsts));
}

/// A stored ZIP of `files`, as Info-ZIP writes one without data descriptors.
fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let crc = crc32fast::hash(data);
        let at = out.len() as u32;
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes());
            }
            h.extend_from_slice(&20u16.to_le_bytes()); // version needed
            h.extend_from_slice(&0u16.to_le_bytes()); // flags
            h.extend_from_slice(&0u16.to_le_bytes()); // stored
            h.extend_from_slice(&0u16.to_le_bytes()); // time
            h.extend_from_slice(&0x21u16.to_le_bytes()); // date
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes()); // extra
            if central {
                h.extend_from_slice(&0u16.to_le_bytes()); // comment
                h.extend_from_slice(&0u16.to_le_bytes()); // disk
                h.extend_from_slice(&0u16.to_le_bytes()); // internal
                h.extend_from_slice(&0u32.to_le_bytes()); // external
                h.extend_from_slice(&at.to_le_bytes());
            }
            h.extend_from_slice(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend_from_slice(data);
        central.extend(header(0x0201_4b50, true));
    }
    let cd_at = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn payload(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

/// A download feed over a folder: a volume is ready once its file is there.
struct DiskFeed {
    finished: Arc<AtomicBool>,
    damaged: Vec<(PathBuf, Vec<(u64, u64)>)>,
}

impl VolumeFeed for DiskFeed {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if let Some((_, holes)) = self.damaged.iter().find(|(p, _)| p == path) {
            return VolumeView::Ready {
                holes: holes.clone(),
            };
        }
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else if self.finished.load(Ordering::Acquire) {
            VolumeView::Missing
        } else {
            VolumeView::NotYet
        }
    }
    fn download_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
    fn set_became_final(&self) -> bool {
        self.finished.load(Ordering::Acquire) && self.damaged.is_empty()
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rg-livefeed-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("dl")).unwrap();
    dir
}

/// Writes `volumes` into `dir` one after another, in `order`, while the extraction runs.
fn arrive(
    dir: PathBuf,
    volumes: Vec<(String, Vec<u8>)>,
    order: Vec<usize>,
    finished: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for i in order {
            std::thread::sleep(Duration::from_millis(30));
            let (name, data) = &volumes[i];
            let tmp = dir.join(format!(".{name}.tmp"));
            std::fs::write(&tmp, data).unwrap();
            std::fs::rename(&tmp, dir.join(name)).unwrap();
        }
        finished.store(true, Ordering::Release);
    })
}

fn run(dir: &Path, set: &LiveSet, feed: &mut DiskFeed) -> (Outcome, Vec<String>) {
    let dest = dir.join("out");
    let mut lines = Vec::new();
    let req = Request {
        dir: &dir.join("dl"),
        set,
        password: "",
    };
    let outcome = extract(
        &req,
        feed,
        &mut || Some((dest.clone(), None)),
        &mut |_, m| lines.push(m.to_string()),
        &mut || false,
        &|_| {},
    );
    (outcome, lines)
}

#[test]
fn a_byte_split_zip_is_extracted_as_its_volumes_arrive() {
    let dir = scratch("zipbytes");
    let movie = payload(700_000, 7);
    let nfo = payload(3_000, 9);
    let zip = stored_zip(&[("movie.mkv", &movie), ("info.nfo", &nfo)]);
    let pieces: Vec<(String, Vec<u8>)> = zip
        .chunks(200_000)
        .enumerate()
        .map(|(i, c)| (format!("Movie.zip.{:03}", i + 1), c.to_vec()))
        .collect();
    let set = live_set(
        b"Movie.zip.001",
        &pieces
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(set.kind, SetKind::ZipBytes);
    let finished = Arc::new(AtomicBool::new(false));
    // the last volume (the directory) first, then in order
    let mut order = vec![pieces.len() - 1];
    order.extend(0..pieces.len() - 1);
    let t = arrive(dir.join("dl"), pieces, order, finished.clone());
    let mut feed = DiskFeed {
        finished,
        damaged: Vec::new(),
    };
    let (outcome, lines) = run(&dir, &set, &mut feed);
    t.join().unwrap();
    assert_eq!(
        outcome,
        Outcome::Done {
            files: 2,
            dirs: 0,
            bytes: (movie.len() + nfo.len()) as u64
        },
        "{lines:?}"
    );
    assert_eq!(std::fs::read(dir.join("out/movie.mkv")).unwrap(), movie);
    assert_eq!(std::fs::read(dir.join("out/info.nfo")).unwrap(), nfo);
    assert!(lines
        .iter()
        .any(|l| l == "Unrar: Extracting from Movie.zip.001"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_split_file_is_joined_as_its_pieces_arrive() {
    let dir = scratch("split");
    let movie = payload(1_050_000, 3);
    let pieces: Vec<(String, Vec<u8>)> = movie
        .chunks(250_000)
        .enumerate()
        .map(|(i, c)| (format!("movie.mkv.{:03}", i + 1), c.to_vec()))
        .collect();
    let set = live_set(
        b"movie.mkv.001",
        &pieces
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let finished = Arc::new(AtomicBool::new(false));
    // out of order: the last (short) piece first, then backwards
    let order: Vec<usize> = (0..pieces.len()).rev().collect();
    let t = arrive(dir.join("dl"), pieces, order, finished.clone());
    let mut feed = DiskFeed {
        finished,
        damaged: Vec::new(),
    };
    let (outcome, lines) = run(&dir, &set, &mut feed);
    t.join().unwrap();
    assert_eq!(
        outcome,
        Outcome::Done {
            files: 1,
            dirs: 0,
            bytes: movie.len() as u64
        },
        "{lines:?}"
    );
    assert_eq!(std::fs::read(dir.join("out/movie.mkv")).unwrap(), movie);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Joined movie.mkv from 5 pieces")),
        "{lines:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_split_with_a_piece_of_another_size_fails_and_leaves_nothing() {
    let dir = scratch("split-bad");
    let mut pieces: Vec<(String, Vec<u8>)> = (0..3)
        .map(|i| (format!("x.bin.{:03}", i + 1), payload(1000, i)))
        .collect();
    pieces[1].1.truncate(900);
    for (n, d) in &pieces {
        std::fs::write(dir.join("dl").join(n), d).unwrap();
    }
    let set = live_set(
        b"x.bin.001",
        &pieces
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut feed = DiskFeed {
        finished: Arc::new(AtomicBool::new(true)),
        damaged: Vec::new(),
    };
    let (outcome, _) = run(&dir, &set, &mut feed);
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    let left: Vec<_> = std::fs::read_dir(dir.join("out")).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_hole_in_a_stored_member_is_read_past_and_filled_after_its_repair() {
    let dir = scratch("zip-hole");
    let movie = payload(600_000, 11);
    let zip = stored_zip(&[("movie.mkv", &movie)]);
    let pieces: Vec<(String, Vec<u8>)> = zip
        .chunks(250_000)
        .enumerate()
        .map(|(i, c)| (format!("m.zip.{:03}", i + 1), c.to_vec()))
        .collect();
    let dl = dir.join("dl");
    for (n, d) in &pieces {
        std::fs::write(dl.join(n), d).unwrap();
    }
    let set = live_set(
        b"m.zip.001",
        &pieces
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    // The middle volume lost 10,000 bytes, zeroed on disk, until its repair.
    let damaged = dl.join("m.zip.002");
    let mut lost = pieces[1].1.clone();
    lost[100_000..110_000].fill(0);
    std::fs::write(&damaged, &lost).unwrap();
    let finished = Arc::new(AtomicBool::new(true));
    let repaired = pieces[1].1.clone();
    let repair_path = damaged.clone();
    struct Repairing {
        inner: DiskFeed,
        polls: usize,
        repaired: Vec<u8>,
        path: PathBuf,
    }
    impl VolumeFeed for Repairing {
        fn volume(&mut self, path: &Path) -> VolumeView {
            self.polls += 1;
            if self.polls == 20 {
                std::fs::write(&self.path, &self.repaired).unwrap();
                self.inner.damaged.clear();
            }
            self.inner.volume(path)
        }
        fn download_finished(&self) -> bool {
            true
        }
        fn set_became_final(&self) -> bool {
            self.inner.set_became_final()
        }
    }
    let mut feed = Repairing {
        inner: DiskFeed {
            finished,
            damaged: vec![(damaged.clone(), vec![(100_000, 10_000)])],
        },
        polls: 0,
        repaired,
        path: repair_path,
    };
    let dest = dir.join("out");
    let req = Request {
        dir: &dl,
        set: &set,
        password: "",
    };
    let outcome = extract(
        &req,
        &mut feed,
        &mut || Some((dest.clone(), None)),
        &mut |_, _| {},
        &mut || false,
        &|_| {},
    );
    assert!(
        matches!(outcome, Outcome::Done { files: 1, .. }),
        "{outcome:?}"
    );
    assert_eq!(std::fs::read(dest.join("movie.mkv")).unwrap(), movie);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_split_numbered_from_zero_is_left_to_post_processing() {
    let all = names(&["m.mkv.000", "m.mkv.001", "m.mkv.002"]);
    assert!(live_set(b"m.mkv.001", &all).is_none());
    let firsts: Vec<&[u8]> = vec![b"m.mkv.001"];
    assert!(
        !in_extracted_set(b"m.mkv.000", &firsts),
        "piece 0 is never another set's"
    );
    assert!(is_infozip_last(b"dir/m.zip", &names(&["m.z01", "m.z02"])));
    assert!(!is_infozip_last(b"m.zip", &names(&["other.z01"])));
    assert!(is_split_first(b"m.mkv.001") && !is_split_first(b"m.7z.001"));
}

#[test]
fn split_pieces_that_are_rar_volumes_are_not_joined() {
    let dir = scratch("split-rar");
    let mut first = b"Rar!\x1a\x07\x00".to_vec();
    first.extend(payload(1000, 1));
    std::fs::write(dir.join("dl/x.avi.001"), &first).unwrap();
    std::fs::write(dir.join("dl/x.avi.002"), payload(500, 2)).unwrap();
    let set = live_set(b"x.avi.001", &names(&["x.avi.001", "x.avi.002"])).unwrap();
    let mut feed = DiskFeed {
        finished: Arc::new(AtomicBool::new(true)),
        damaged: Vec::new(),
    };
    let (outcome, _) = run(&dir, &set, &mut feed);
    assert!(matches!(outcome, Outcome::Declined(_)), "{outcome:?}");
    assert!(
        !dir.join("out").exists(),
        "nothing prepared, nothing written"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_that_died_leaves_a_temp_file_the_next_run_replaces() {
    let dir = scratch("stale-temp");
    let movie = payload(300_000, 5);
    let pieces: Vec<(String, Vec<u8>)> = movie
        .chunks(100_000)
        .enumerate()
        .map(|(i, c)| (format!("m.mkv.{:03}", i + 1), c.to_vec()))
        .collect();
    for (n, d) in &pieces {
        std::fs::write(dir.join("dl").join(n), d).unwrap();
    }
    // What a killed run left in the unpack folder.
    std::fs::create_dir_all(dir.join("out/.rustyget-live")).unwrap();
    std::fs::write(dir.join("out/.rustyget-live/0.part"), b"stale").unwrap();
    let set = live_set(
        b"m.mkv.001",
        &pieces
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut feed = DiskFeed {
        finished: Arc::new(AtomicBool::new(true)),
        damaged: Vec::new(),
    };
    let (outcome, _) = run(&dir, &set, &mut feed);
    assert!(
        matches!(outcome, Outcome::Done { files: 1, .. }),
        "{outcome:?}"
    );
    let left: Vec<_> = std::fs::read_dir(dir.join("out"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, vec![std::ffi::OsString::from("m.mkv")]);
    assert_eq!(std::fs::read(dir.join("out/m.mkv")).unwrap(), movie);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_output_gives_back_what_was_written_to_it() {
    let dir = scratch("held");
    let path = dir.join("out.bin");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let mut out = OutFile {
        file,
        written: Ranges::default(),
    };
    out.write_at(100, b"hello").unwrap();
    out.write_at(105, b" world").unwrap();
    let mut buf = [0u8; 32];
    assert_eq!(out.read_held(100, &mut buf).unwrap(), 11);
    assert_eq!(&buf[..11], b"hello world");
    assert_eq!(out.read_held(103, &mut buf[..4]).unwrap(), 4);
    assert_eq!(&buf[..4], b"lo w");
    assert_eq!(out.read_held(0, &mut buf).unwrap(), 0, "nothing written there");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_member_named_like_the_temp_folder_is_left_to_post_processing() {
    assert!(in_temp_dir(Path::new(".rustyget-live/0.part")));
    assert!(in_temp_dir(Path::new(".RustyGet-Live")));
    assert!(!in_temp_dir(Path::new("movie/.rustyget-live")));
    assert!(!in_temp_dir(Path::new("movie.mkv")));
}

/// A download feed whose volumes not on disk yet may be being written (see `crate::livewrite`).
struct WrittenDiskFeed {
    disk: DiskFeed,
    nzb_id: i32,
}

impl VolumeFeed for WrittenDiskFeed {
    fn volume(&mut self, path: &Path) -> VolumeView {
        self.disk.volume(path)
    }
    fn download_finished(&self) -> bool {
        self.disk.download_finished()
    }
    fn set_became_final(&self) -> bool {
        self.disk.set_became_final()
    }
    fn growing(&mut self, path: &Path) -> Option<Arc<crate::livewrite::LiveVolume>> {
        if path.is_file() {
            return None;
        }
        crate::livewrite::lookup(self.nzb_id, crate::fsops::path_to_bytes(path))
    }
}

#[test]
fn volumes_being_written_are_fed_article_by_article() {
    use std::os::unix::fs::FileExt;
    let dir = scratch("written");
    let dl = dir.join("dl");
    // The last piece holds the whole 64 KiB the central directory is searched in.
    let movie = payload(1_300_000, 21);
    let zip = stored_zip(&[("movie.mkv", &movie)]);
    let pieces: Vec<(String, Vec<u8>)> = zip
        .chunks(200_000)
        .enumerate()
        .map(|(i, c)| (format!("Movie.zip.{:03}", i + 1), c.to_vec()))
        .collect();
    let names: Vec<Vec<u8>> = pieces.iter().map(|(n, _)| n.as_bytes().to_vec()).collect();
    let set = live_set(b"Movie.zip.001", &names).unwrap();
    let n = pieces.len();
    // The first and the last volume (the directory) are on disk: the set maps at once.
    std::fs::write(dl.join(&pieces[0].0), &pieces[0].1).unwrap();
    std::fs::write(dl.join(&pieces[n - 1].0), &pieces[n - 1].1).unwrap();
    let nzb_id = -7_330_001;
    let finished = Arc::new(AtomicBool::new(false));
    let fin = finished.clone();
    let wdl = dl.clone();
    let middle: Vec<(String, Vec<u8>)> = pieces[1..n - 1].to_vec();
    let writer = std::thread::spawn(move || {
        for (k, (name, data)) in middle.iter().enumerate() {
            let out = wdl.join(format!("{k}.out.tmp"));
            let f = std::fs::File::create(&out).unwrap();
            f.set_len(data.len() as u64).unwrap();
            let target = wdl.join(name);
            crate::livewrite::register(
                nzb_id,
                k as i32,
                crate::fsops::path_to_bytes(&target),
                crate::fsops::path_to_bytes(&out),
                data.len() as u64,
            );
            // Articles in reverse.
            let starts: Vec<usize> = (0..data.len()).step_by(10_000).rev().collect();
            for s in starts {
                let e = (s + 10_000).min(data.len());
                f.write_all_at(&data[s..e], s as u64).unwrap();
                crate::livewrite::written(nzb_id, k as i32, s as u64, (e - s) as u64);
                std::thread::sleep(Duration::from_millis(1));
            }
            drop(f);
            std::fs::rename(&out, &target).unwrap();
        }
        crate::livewrite::forget(nzb_id);
        fin.store(true, Ordering::Release);
    });
    let mut feed = WrittenDiskFeed {
        disk: DiskFeed {
            finished,
            damaged: Vec::new(),
        },
        nzb_id,
    };
    let dest = dir.join("out");
    let mut lines = Vec::new();
    let req = Request {
        dir: &dl,
        set: &set,
        password: "",
    };
    let before = crate::livewrite::read_counts();
    let outcome = extract(
        &req,
        &mut feed,
        &mut || Some((dest.clone(), None)),
        &mut |_, m| lines.push(m.to_string()),
        &mut || false,
        &|_| {},
    );
    writer.join().unwrap();
    let after = crate::livewrite::read_counts();
    assert_eq!(
        outcome,
        Outcome::Done {
            files: 1,
            dirs: 0,
            bytes: movie.len() as u64
        },
        "{lines:?}"
    );
    assert_eq!(std::fs::read(dir.join("out/movie.mkv")).unwrap(), movie);
    assert!(after.0 > before.0, "nothing was read as written: {before:?} {after:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
