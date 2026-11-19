//! PAR2 verify and repair through `postproc::NzbgetRsParChecker`, against the
//! fixture in `tests/testdata/parchecker`. Ported from the C++ unit test
//! `tests/postprocess/ParCheckerTest.h`.
#![cfg(unix)]

use std::cell::RefCell;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use nzbget_rs::postproc::{NzbgetRsParChecker, ParCheckerHooks, ParCheckerOptions};
use par_rs::QuickInfo;

const PS_FAILED: i32 = 0;
const PS_REPAIR_POSSIBLE: i32 = 1;
const PS_REPAIRED: i32 = 2;
const PS_REPAIR_NOT_NEEDED: i32 = 3;

/// One copy of the fixture in a fresh directory, plus what the checker
/// reports back through its hooks.
struct Checker {
    dir: PathBuf,
    repair: bool,
    ignore_ext: String,
    verified: RefCell<Vec<String>>,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/testdata/parchecker")
}

impl Checker {
    fn new(name: &str, repair: bool) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-{}-{}-{}",
            name,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for entry in fs::read_dir(fixture_dir()).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), dir.join(entry.file_name())).unwrap();
        }
        Self {
            dir,
            repair,
            ignore_ext: String::new(),
            verified: RefCell::default(),
        }
    }

    fn ignore_ext(mut self, value: &str) -> Self {
        self.ignore_ext = value.to_owned();
        self
    }

    fn corrupt(&self, name: &str, offset: u64) {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(self.dir.join(name))
            .unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(&[0]).unwrap();
    }

    /// Runs verification (and repair when enabled). Returns (status, par_full).
    fn execute(&self, quick: bool) -> (i32, bool) {
        let checker = NzbgetRsParChecker::new();
        checker.set_dest_dir(self.dir.to_str().unwrap().as_bytes().to_vec());
        checker.set_par_quick(quick);
        checker.execute(self);
        (checker.status(), checker.par_full())
    }

    fn verified(&self) -> Vec<String> {
        self.verified.borrow().clone()
    }

    /// The CRC `crc.txt` lists for `name`, and whether the file still matches it.
    fn find_crc(&self, name: &str) -> Option<(u32, bool)> {
        let list = fs::read_to_string(self.dir.join("crc.txt")).ok()?;
        let mut words = list.split_whitespace();
        while let (Some(file), Some(crc)) = (words.next(), words.next()) {
            if file == name {
                let expected = u32::from_str_radix(crc, 16).ok()?;
                let actual = crc32fast::hash(&fs::read(self.dir.join(name)).ok()?);
                return Some((expected, expected == actual));
            }
        }
        None
    }
}

/// The daemon's side of the checker, as the C++ test wrapper answered it:
/// fixed options, no extra pars to fetch, verified files recorded, and a
/// known CRC only for files that still match `crc.txt`.
impl ParCheckerHooks for Checker {
    fn read_options(&self) -> ParCheckerOptions {
        ParCheckerOptions {
            par_buffer_mb: 250, // ParBuffer default
            has_memory_monitor: false,
            par_threads: 0, // ParThreads default
            par_scan: 1,    // ParScan=extended
            par_repair: self.repair,
            par_ignore_ext: self.ignore_ext.as_bytes().to_vec(),
        }
    }

    fn request_more_pars(&self, _block_needed: i32) -> (bool, i32) {
        (false, 0)
    }

    fn register_verified_file(&self, filename: &[u8]) {
        let name = String::from_utf8_lossy(filename);
        let base = Path::new(name.as_ref())
            .file_name()
            .map_or_else(|| name.to_string(), |b| b.to_string_lossy().into_owned());
        self.verified.borrow_mut().push(base);
    }

    fn find_file_quick_info(&self, filename: &[u8]) -> Option<QuickInfo> {
        match self.find_crc(std::str::from_utf8(filename).ok()?) {
            Some((crc, true)) => Some(QuickInfo::Success { crc }),
            _ => None,
        }
    }
}

impl Drop for Checker {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn repair_not_needed() {
    let c = Checker::new("noneed", false);
    assert_eq!(c.execute(false), (PS_REPAIR_NOT_NEEDED, true));
}

#[test]
fn verified_targets_exclude_unprotected_files() {
    let c = Checker::new("unprotected", true);
    fs::write(
        c.dir.join("unprotected.bin"),
        b"This file has no matching PAR member",
    )
    .unwrap();
    c.corrupt("testfile.dat", 20000);
    assert_eq!(c.execute(false).0, PS_REPAIRED);
    let mut verified = c.verified();
    verified.sort();
    assert_eq!(verified, ["testfile.dat", "testfile.nfo"]);
}

#[test]
fn verified_targets_exclude_ignored_damage() {
    // libpar2 may report an empty name for damaged files. A wildcard keeps
    // this fixture about verified membership, independent of that callback.
    let c = Checker::new("ignored", false).ignore_ext("*");
    c.corrupt("testfile.dat", 20000);
    assert_eq!(c.execute(false).0, PS_REPAIR_NOT_NEEDED);
    assert_eq!(c.verified(), ["testfile.nfo"]);
}

#[test]
fn repair_possible() {
    let c = Checker::new("possible", false);
    c.corrupt("testfile.dat", 20000);
    assert_eq!(c.execute(false), (PS_REPAIR_POSSIBLE, true));
}

#[test]
fn repair_successful() {
    let c = Checker::new("successful", true);
    c.corrupt("testfile.dat", 20000);
    assert_eq!(c.execute(false), (PS_REPAIRED, true));
}

fn corrupt_beyond_repair(c: &Checker) {
    for offset in (20000..=80000).step_by(10000) {
        c.corrupt("testfile.dat", offset);
    }
}

#[test]
fn repair_failed() {
    let c = Checker::new("failed", false);
    corrupt_beyond_repair(&c);
    assert_eq!(c.execute(false), (PS_FAILED, true));
}

#[test]
fn repair_failed_even_with_repair_enabled() {
    // Was ParIgnoreExtDatTest / ExtCleanupDiskDatTest: ExtCleanupDisk is not
    // part of the checker's options, so both only proved this.
    let c = Checker::new("failedrepair", true);
    corrupt_beyond_repair(&c);
    assert_eq!(c.execute(false).0, PS_FAILED);
}

#[test]
fn quick_verification_repair_not_needed() {
    let c = Checker::new("quicknoneed", false);
    assert_eq!(c.execute(true), (PS_REPAIR_NOT_NEEDED, false));
}

#[test]
fn quick_verification_repair_successful() {
    let c = Checker::new("quickrepair", true);
    c.corrupt("testfile.dat", 20000);
    assert_eq!(c.execute(true), (PS_REPAIRED, false));
}

#[test]
fn quick_full_verification_repair_successful() {
    // Every file is damaged, so quick verification falls back to a full one.
    let c = Checker::new("quickfull", true);
    c.corrupt("testfile.dat", 20000);
    c.corrupt("testfile.nfo", 100);
    assert_eq!(c.execute(true), (PS_REPAIRED, true));
}
