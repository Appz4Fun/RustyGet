//! PAR2 verify and repair through the par checker C ABI, against the fixture in
//! `tests/testdata/parchecker`. Ported from the C++ unit test
//! `tests/postprocess/ParCheckerTest.h`, which drove the same ABI through the
//! C++ `ParChecker` wrapper.
#![cfg(unix)]

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use nzbget_rs::ffi::{
    nzbget_rs_par_checker_exec, nzbget_rs_par_checker_free, nzbget_rs_par_checker_new,
};

const PS_FAILED: c_int = 0;
const PS_REPAIR_POSSIBLE: c_int = 1;
const PS_REPAIRED: c_int = 2;
const PS_REPAIR_NOT_NEEDED: c_int = 3;

const FS_UNKNOWN: usize = 0;
const FS_SUCCESS: usize = 1;

/// Option values the daemon hands over in host operation 0.
struct Config {
    repair: bool,
    ignore_ext: CString,
}

/// One copy of the fixture in a fresh directory, plus what the host callback
/// records.
struct Checker {
    dir: PathBuf,
    config: Config,
    verified: Vec<String>,
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
            config: Config {
                repair,
                ignore_ext: CString::default(),
            },
            verified: Vec::new(),
        }
    }

    fn ignore_ext(mut self, value: &str) -> Self {
        self.config.ignore_ext = CString::new(value).unwrap();
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
    fn execute(&mut self, quick: bool) -> (c_int, bool) {
        let dest = CString::new(self.dir.to_str().unwrap()).unwrap();
        unsafe {
            let ctrl = nzbget_rs_par_checker_new();
            nzbget_rs_par_checker_exec(ctrl, 1, 0, dest.as_ptr(), None, std::ptr::null_mut());
            nzbget_rs_par_checker_exec(
                ctrl,
                2,
                c_int::from(quick),
                std::ptr::null(),
                None,
                std::ptr::null_mut(),
            );
            nzbget_rs_par_checker_exec(
                ctrl,
                0,
                0,
                std::ptr::null(),
                Some(host),
                (self as *mut Checker).cast(),
            );
            let full = nzbget_rs_par_checker_exec(
                ctrl,
                3,
                0,
                std::ptr::null(),
                None,
                std::ptr::null_mut(),
            ) != 0;
            let status = nzbget_rs_par_checker_exec(
                ctrl,
                4,
                0,
                std::ptr::null(),
                None,
                std::ptr::null_mut(),
            );
            nzbget_rs_par_checker_free(ctrl);
            (status, full)
        }
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

impl Drop for Checker {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

unsafe fn text<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
    }
}

/// Host callback, as the C++ wrapper answered it: 0 options, 1 request more
/// pars (none available), 2 verified file, 3 known file CRC, 4 log.
unsafe extern "C" fn host(
    ctx: *mut c_void,
    op: c_int,
    _a1: c_int,
    _a2: c_int,
    p1: usize,
    _p2: usize,
    s1: *const c_char,
    _s2: *const c_char,
    ints: *mut c_int,
    _i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    let checker = unsafe { &mut *ctx.cast::<Checker>() };
    match op {
        0 => unsafe {
            *ints = 250; // ParBuffer default
            *ints.add(1) = 0;
            *ints.add(2) = 0; // ParThreads default
            *ints.add(3) = 1; // ParScan=extended
            *ints.add(4) = c_int::from(checker.config.repair);
            *strs = checker.config.ignore_ext.as_ptr();
            0
        },
        1 => {
            unsafe { *ints = 0 };
            0
        }
        2 => {
            let name = unsafe { text(s1) };
            let base = Path::new(name)
                .file_name()
                .map_or_else(|| name.to_owned(), |b| b.to_string_lossy().into_owned());
            checker.verified.push(base);
            0
        }
        3 => match checker.find_crc(unsafe { text(s1) }) {
            Some((crc, matches)) => {
                unsafe { *(p1 as *mut u32) = crc };
                if matches {
                    FS_SUCCESS
                } else {
                    FS_UNKNOWN
                }
            }
            None => FS_UNKNOWN,
        },
        _ => 0,
    }
}

#[test]
fn repair_not_needed() {
    let mut c = Checker::new("noneed", false);
    assert_eq!(c.execute(false), (PS_REPAIR_NOT_NEEDED, true));
}

#[test]
fn verified_targets_exclude_unprotected_files() {
    let mut c = Checker::new("unprotected", true);
    fs::write(
        c.dir.join("unprotected.bin"),
        b"This file has no matching PAR member",
    )
    .unwrap();
    c.corrupt("testfile.dat", 20000);
    assert_eq!(c.execute(false).0, PS_REPAIRED);
    let mut verified = c.verified.clone();
    verified.sort();
    assert_eq!(verified, ["testfile.dat", "testfile.nfo"]);
}

#[test]
fn verified_targets_exclude_ignored_damage() {
    // libpar2 may report an empty name for damaged files. A wildcard keeps
    // this fixture about verified membership, independent of that callback.
    let mut c = Checker::new("ignored", false).ignore_ext("*");
    c.corrupt("testfile.dat", 20000);
    assert_eq!(c.execute(false).0, PS_REPAIR_NOT_NEEDED);
    assert_eq!(c.verified, ["testfile.nfo"]);
}

#[test]
fn repair_possible() {
    let mut c = Checker::new("possible", false);
    c.corrupt("testfile.dat", 20000);
    assert_eq!(c.execute(false), (PS_REPAIR_POSSIBLE, true));
}

#[test]
fn repair_successful() {
    let mut c = Checker::new("successful", true);
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
    let mut c = Checker::new("failed", false);
    corrupt_beyond_repair(&c);
    assert_eq!(c.execute(false), (PS_FAILED, true));
}

#[test]
fn repair_failed_even_with_repair_enabled() {
    // Was ParIgnoreExtDatTest / ExtCleanupDiskDatTest: ExtCleanupDisk is not
    // part of the checker's options, so both only proved this.
    let mut c = Checker::new("failedrepair", true);
    corrupt_beyond_repair(&c);
    assert_eq!(c.execute(false).0, PS_FAILED);
}

#[test]
fn quick_verification_repair_not_needed() {
    let mut c = Checker::new("quicknoneed", false);
    assert_eq!(c.execute(true), (PS_REPAIR_NOT_NEEDED, false));
}

#[test]
fn quick_verification_repair_successful() {
    let mut c = Checker::new("quickrepair", true);
    c.corrupt("testfile.dat", 20000);
    assert_eq!(c.execute(true), (PS_REPAIRED, false));
}

#[test]
fn quick_full_verification_repair_successful() {
    // Every file is damaged, so quick verification falls back to a full one.
    let mut c = Checker::new("quickfull", true);
    c.corrupt("testfile.dat", 20000);
    c.corrupt("testfile.nfo", 100);
    assert_eq!(c.execute(true), (PS_REPAIRED, true));
}
