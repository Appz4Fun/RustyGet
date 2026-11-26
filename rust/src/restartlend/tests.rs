use super::*;

/// A tool on PATH (rar, 7z, 7zz), or None: the tests that need one skip without it.
fn tool(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Compressible bytes with incompressible stretches, as a video's headers and frames mix.
fn payload(size: usize, seed: u64) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let words: Vec<Vec<u8>> = (0..500)
        .map(|_| {
            (0..(2 + next() % 8))
                .map(|_| b'a' + (next() % 26) as u8)
                .collect()
        })
        .collect();
    let mut out = Vec::with_capacity(size + 64);
    while out.len() < size {
        if next() % 7 == 0 {
            for _ in 0..(next() % 4096) {
                out.push(next() as u8);
            }
        } else {
            for _ in 0..12 {
                out.extend_from_slice(&words[(next() % 500) as usize]);
                out.push(b' ');
            }
            out.push(b'\n');
        }
    }
    out.truncate(size);
    out
}

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rg-restartlend-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    /// The volumes `tool` wrote into the fixture, in set order (`out.partN.rar` or `out.7z.NNN`).
    fn volumes(&self, prefix: &str) -> Vec<(String, Vec<u8>)> {
        let mut names: Vec<String> = std::fs::read_dir(&self.dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with(prefix))
            .collect();
        names.sort_by_key(|n| {
            n.chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        });
        names
            .into_iter()
            .map(|n| (n.clone(), std::fs::read(self.dir.join(&n)).unwrap()))
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Restarts `vols` at `frontier` with `movie` as the download's own output; also counts the
/// volume bytes read.
fn run(
    vols: &[(String, Vec<u8>)],
    movie: &[u8],
    frontier: u64,
    scratch: &Path,
) -> (Result<Restarted, String>, u64) {
    let sizes: Vec<u64> = vols.iter().map(|(_, v)| v.len() as u64).collect();
    let mut read_bytes = 0u64;
    let mut read = |volume: u32, offset: u64, buf: &mut [u8]| -> bool {
        let Some((_, v)) = vols.get(volume as usize) else {
            return false;
        };
        let Some(src) = v.get(offset as usize..offset as usize + buf.len()) else {
            return false;
        };
        buf.copy_from_slice(src);
        read_bytes += buf.len() as u64;
        true
    };
    let mut known = |offset: u64, buf: &mut [u8]| -> bool {
        // only the output before the frontier is the download's
        if offset + buf.len() as u64 > frontier {
            return false;
        }
        buf.copy_from_slice(&movie[offset as usize..offset as usize + buf.len()]);
        true
    };
    let first = vols[0].0.clone();
    let mut backup = Backup {
        first_name: first.as_bytes(),
        sizes: &sizes,
        password: None,
        read: &mut read,
        prefetch: &mut |_: &[(u32, u64, u64)]| {},
    };
    let r = restart(
        &mut backup,
        movie.len() as u64,
        frontier,
        &mut known,
        scratch,
    );
    (r, read_bytes)
}

fn check_decoded(r: &Restarted, movie: &[u8], frontier: u64) {
    assert!(
        r.from <= frontier - OVERLAP,
        "restart {} not before the overlap",
        r.from
    );
    let mut got = vec![0u8; movie.len() - r.from as usize];
    assert!(r.read(r.from, &mut got));
    assert!(got == movie[r.from as usize..], "decoded output differs");
    assert!(!r.read(r.from.saturating_sub(1), &mut [0u8; 2]) || r.from == 0);
}

#[test]
fn rar5_compressed_backup_restarts_near_the_frontier_and_reads_only_the_rest() {
    let Some(rar) = tool(&["rar"]) else { return };
    let fx = Fixture::new("rar5");
    let movie = payload(6_000_000, 7);
    std::fs::write(fx.dir.join("movie.mkv"), &movie).unwrap();
    let ok = std::process::Command::new(rar)
        .args([
            "a",
            "-idq",
            "-m3",
            "-ma5",
            "-md1m",
            "-v1000000b",
            "out.rar",
            "movie.mkv",
        ])
        .current_dir(&fx.dir)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    std::fs::remove_file(fx.dir.join("movie.mkv")).unwrap();
    let vols = fx.volumes("out.part");
    let total: u64 = vols.iter().map(|(_, v)| v.len() as u64).sum();
    let frontier = 4_500_000;
    let (r, read) = run(&vols, &movie, frontier, &fx.dir);
    let r = r.expect("restarted");
    assert_eq!(r.layer, "RAR5");
    check_decoded(&r, &movie, frontier);
    // the packed bytes from about the frontier's position less the search region, and the
    // headers: well under the whole set
    assert!(read < total * 2 / 3, "read {read} of {total}");
    assert_eq!(r.fetched, read);
    let path = r.path.clone();
    drop(r);
    assert!(
        !path.exists(),
        "the scratch file is removed with the restart"
    );
}

#[test]
fn lzma2_backup_restarts_at_a_chunk_before_the_frontier() {
    let Some(sz) = tool(&["7zz", "7z", "7za"]) else {
        return;
    };
    let fx = Fixture::new("lzma2");
    let movie = payload(14_000_000, 11);
    std::fs::write(fx.dir.join("movie.mkv"), &movie).unwrap();
    let ok = std::process::Command::new(sz)
        .args([
            "a",
            "-bd",
            "-m0=LZMA2:d=1m",
            "-v1000000b",
            "out.7z",
            "movie.mkv",
        ])
        .current_dir(&fx.dir)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok);
    std::fs::remove_file(fx.dir.join("movie.mkv")).unwrap();
    let vols = fx.volumes("out.7z.");
    let total: u64 = vols.iter().map(|(_, v)| v.len() as u64).sum();
    // far enough in that the restart's search region (4 MiB) leaves the chunks before it to the walk
    let frontier = 12_000_000;
    let (r, read) = run(&vols, &movie, frontier, &fx.dir);
    let r = r.expect("restarted");
    assert_eq!(r.layer, "LZMA2");
    check_decoded(&r, &movie, frontier);
    // never a byte read twice: at most the whole set
    assert!(read <= total, "read {read} of {total}");
}

#[test]
fn a_coder_with_no_restart_point_says_why() {
    let Some(sz) = tool(&["7zz", "7z", "7za"]) else {
        return;
    };
    let fx = Fixture::new("bzip2");
    let movie = payload(3_000_000, 13);
    std::fs::write(fx.dir.join("movie.mkv"), &movie).unwrap();
    let ok = std::process::Command::new(sz)
        .args(["a", "-bd", "-m0=BZip2", "-v1000000b", "out.7z", "movie.mkv"])
        .current_dir(&fx.dir)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok);
    std::fs::remove_file(fx.dir.join("movie.mkv")).unwrap();
    let vols = fx.volumes("out.7z.");
    let (r, _) = run(&vols, &movie, 2_000_000, &fx.dir);
    let why = r.expect_err("BZip2 restarts only at its start");
    assert!(why.contains("restarts only at its start"), "{why}");
    // nothing left behind
    assert!(fx.volumes(".rg-restart").is_empty());
}

#[test]
fn a_backup_of_another_size_is_refused_before_any_decoding() {
    let Some(rar) = tool(&["rar"]) else { return };
    let fx = Fixture::new("size");
    let movie = payload(2_000_000, 17);
    std::fs::write(fx.dir.join("movie.mkv"), &movie).unwrap();
    assert!(std::process::Command::new(rar)
        .args([
            "a",
            "-idq",
            "-m3",
            "-ma5",
            "-v1000000b",
            "out.rar",
            "movie.mkv"
        ])
        .current_dir(&fx.dir)
        .status()
        .unwrap()
        .success());
    std::fs::remove_file(fx.dir.join("movie.mkv")).unwrap();
    let vols = fx.volumes("out.part");
    let other = payload(2_000_001, 17);
    let (r, _) = run(&vols, &other, 1_500_000, &fx.dir);
    assert!(r
        .expect_err("no member of that size")
        .contains("no member of the video's size"));
}
