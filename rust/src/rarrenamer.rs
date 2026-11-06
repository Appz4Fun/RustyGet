//! RarRenamer's renaming (RarRenamer.cpp, RenameFiles and what it uses): the
//! rar volumes of a directory gathered into complete sets (MakeSets), the sets
//! not properly named (IsSetProperlyNamed) renamed after their largest file
//! (FindMainFile) with new volume names (GenNewVolumeFilename) that don't
//! overwrite other files, exactly as the C++ did. Reading the volumes, the
//! regular expression, the file checks and the moves stay C++ (through `Env`).

use std::ffi::{c_char, c_int};

extern "C" {
    fn atoi(s: *const c_char) -> c_int;
}

/// A file of a volume.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct File {
    pub name: Vec<u8>,
    pub size: i64,
    pub split_before: bool,
    pub split_after: bool,
}

/// A volume read from the directory (RarVolume).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Volume {
    /// the full path
    pub filename: Vec<u8>,
    pub version: i32,
    pub volume_no: u32,
    pub multi_volume: bool,
    pub has_next_volume: bool,
    pub new_naming: bool,
    pub files: Vec<File>,
}

/// The C++ side.
pub trait Env {
    fn file_exists(&mut self, path: &[u8]) -> bool;
    /// RegEx(".*.part([0-9]+)\\.rar$") on a name: group 1's start and length
    fn match_part(&mut self, name: &[u8]) -> Option<(usize, usize)>;
    /// RenameFile(volumes[index]'s name, dest)
    fn rename(&mut self, index: usize, dest: &[u8]);
}

const BSTRING_MAX: usize = 1023;

fn cut(s: &[u8]) -> &[u8] {
    &s[..s.len().min(BSTRING_MAX)]
}

/// The C string of a std::string: up to its first NUL.
fn c(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
}

fn base_name(path: &[u8]) -> &[u8] {
    let p = c(path);
    &p[crate::paths::base_file_name(p)..]
}

fn rfind(s: &[u8], b: u8) -> Option<usize> {
    s.iter().rposition(|&x| x == b)
}

/// strncmp(a, b, n) == 0 on C strings
fn strncmp_eq(a: &[u8], b: &[u8], n: usize) -> bool {
    let (a, b) = (c(a), c(b));
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return false;
        }
        if x == 0 {
            return true;
        }
    }
    true
}

/// GenNewExtension
pub fn new_extension(volume_no: i32) -> Vec<u8> {
    format!("part{:04}.rar", volume_no.wrapping_add(1)).into_bytes()
}

/// GenOldExtension: rar, r00..r99, s00..
pub fn old_extension(volume_no: i32) -> Vec<u8> {
    if volume_no == 0 {
        return b"rar".to_vec();
    }
    let n = volume_no.wrapping_sub(1);
    let ch = (b'r' as i32).wrapping_add(n / 100) as u8;
    let mut v = vec![ch];
    v.extend_from_slice(format!("{:02}", n % 100).as_bytes());
    // "%c" of a NUL ends the C string there
    c(&v).to_vec()
}

fn new_volume_filename(dir: &[u8], base: &[u8], v: &Volume) -> Vec<u8> {
    // the C++ passed the uint32 volume number as an int
    let ext = if v.new_naming { new_extension(v.volume_no as i32) } else { old_extension(v.volume_no as i32) };
    [c(dir), &[crate::paths::PATH_SEPARATOR], c(base), b".", &ext].concat()
}

/// SameArchiveName
pub fn same_archive_name(f1: &[u8], f2: &[u8], new_naming: bool) -> bool {
    let (f1, f2) = (c(f1), c(f2));
    if f1.len() != f2.len() {
        return false;
    }
    let (Some(e1), Some(e2)) = (rfind(f1, b'.'), rfind(f2, b'.')) else { return false };
    if f1.len() - e1 != f2.len() - e2 {
        return false;
    }
    if new_naming {
        if e1 == 0 || e2 == 0 {
            return false;
        }
        let n1 = cut(&f1[..e1]);
        let n2 = cut(&f2[..e2]);
        let (Some(x1), Some(x2)) = (rfind(n1, b'.'), rfind(n2, b'.')) else { return false };
        n1.len() - x1 == n2.len() - x2
            && n1[x1..].starts_with(b".part")
            && n2[x2..].starts_with(b".part")
            && strncmp_eq(n1, n2, x1)
    } else {
        strncmp_eq(f1, f2, e1)
    }
}

/// MakeSets: the complete sets, as volume indexes.
pub fn make_sets(volumes: &[Volume]) -> Vec<Vec<usize>> {
    let mut sets: Vec<Vec<usize>> = volumes
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.files.is_empty() && v.volume_no == 0 && !v.files[0].split_before)
        .map(|(i, _)| vec![i])
        .collect();
    sets.retain_mut(|set| {
        loop {
            let last = &volumes[*set.last().expect("a set starts with a volume")];
            let candidates: Vec<usize> = volumes
                .iter()
                .enumerate()
                .filter(|(_, v)| {
                    !v.files.is_empty()
                        && v.multi_volume
                        && v.volume_no == last.volume_no.wrapping_add(1)
                        && v.version == last.version
                        && last.has_next_volume
                        && ((v.files[0].split_before
                            && last.files.last().is_some_and(|f| f.split_after)
                            && c(&v.files[0].name) == c(&last.files.last().expect("checked").name))
                            || (!v.files[0].split_before && !last.files.last().is_some_and(|f| f.split_after)))
                })
                .map(|(i, _)| i)
                .collect();
            let mut next = None;
            if candidates.len() > 1 {
                let first = &volumes[set[0]];
                next = candidates
                    .iter()
                    .copied()
                    .find(|&i| same_archive_name(base_name(&first.filename), base_name(&volumes[i].filename), first.new_naming));
            }
            if next.is_none() {
                next = candidates.first().copied();
            }
            match next {
                Some(i) => set.push(i),
                None => break,
            }
        }
        let last = &volumes[*set.last().expect("a set starts with a volume")];
        // kept when completed
        !last.has_next_volume && last.files.last().is_none_or(|f| !f.split_after)
    });
    sets
}

/// IsSetProperlyNamed
fn properly_named(volumes: &[Volume], set: &[usize], env: &mut dyn Env) -> bool {
    let set_base = base_name(&volumes[set[0]].filename);
    let mut set_part_len = 0;
    for &i in set {
        let v = &volumes[i];
        let name = base_name(&v.filename);
        if set_base.len() != name.len() {
            return false;
        }
        if v.new_naming {
            let Some((start, _len)) = env.match_part(name) else { return false };
            // BString<1024>'s format constructor: the name from the group on
            // as the format (digits, then ".rar"), the length unused
            let mut part = cut(&name[start.min(name.len())..]).to_vec();
            if set_part_len == 0 {
                set_part_len = part.len();
            }
            let plen = part.len();
            part.push(0);
            let number = unsafe { atoi(part.as_ptr().cast()) } as u32;
            if !(number == v.volume_no.wrapping_add(1) && plen == set_part_len && strncmp_eq(set_base, name, start)) {
                return false;
            }
        } else {
            let Some(dot) = rfind(name, b'.') else { return false };
            if name[dot + 1..] != old_extension(v.volume_no as i32)[..] || !strncmp_eq(set_base, name, dot) {
                return false;
            }
        }
    }
    true
}

/// FindMainFile: the first largest file of the set (None: no files).
fn main_file<'a>(volumes: &'a [Volume], set: &[usize]) -> Option<&'a File> {
    let mut best: Option<&File> = None;
    for &i in set {
        for f in &volumes[i].files {
            // std::max_element: the first of equal largest ones
            if best.is_none_or(|b| b.size < f.size) {
                best = Some(f);
            }
        }
    }
    best
}

/// RenameFiles for the volumes of `dir`.
pub fn rename_files(dir: &[u8], volumes: &[Volume], env: &mut dyn Env) {
    for set in make_sets(volumes) {
        if properly_named(volumes, &set, env) {
            continue;
        }
        // a set starts with a volume that has files: there is a main file
        let Some(main) = main_file(volumes, &set) else { continue };
        let mut main_base = cut(base_name(&main.name)).to_vec();
        if let Some(dot) = rfind(&main_base, b'.') {
            // strip a three-character extension
            if main_base.len() - dot == 4 {
                main_base.truncate(dot);
            }
        }
        let mut new_base = main_base.clone();
        let mut num = 0;
        loop {
            if num > 0 {
                new_base = cut(&[&main_base[..], b"-", (num + 1).to_string().as_bytes()].concat()).to_vec();
            }
            num += 1;
            let overwrite = set.iter().any(|&i| {
                let dest = new_volume_filename(dir, &new_base, &volumes[i]);
                c(&volumes[i].filename) != &dest[..] && env.file_exists(&dest)
            });
            if !overwrite {
                break;
            }
        }
        for &i in &set {
            let dest = new_volume_filename(dir, &new_base, &volumes[i]);
            if c(&volumes[i].filename) != &dest[..] {
                env.rename(i, &dest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions() {
        assert_eq!(new_extension(0), b"part0001.rar");
        assert_eq!(old_extension(0), b"rar");
        assert_eq!(old_extension(1), b"r00");
        assert_eq!(old_extension(101), b"s00");
        assert!(same_archive_name(b"a.part01.rar", b"a.part02.rar", true));
        assert!(!same_archive_name(b"a.part01.rar", b"b.part02.rar", true));
        assert!(same_archive_name(b"a.r00", b"a.r01", false));
    }

    #[test]
    fn signed_volume_numbers_and_platform_separator() {
        assert_eq!(new_extension(-2), b"part-001.rar");
        assert_eq!(new_extension(i32::MAX), b"part-2147483648.rar");
        assert_eq!(old_extension(-1), b"r-2");
        assert_eq!(old_extension(14_201), b""); // the %c byte wraps to NUL
        assert_eq!(old_extension(14_301), b"\x0100");
        let v = Volume { volume_no: u32::MAX, new_naming: true, ..Volume::default() };
        assert_eq!(new_volume_filename(b"dir", b"movie", &v),
            [b"dir".as_slice(), &[crate::paths::PATH_SEPARATOR], b"movie.part0000.rar"].concat());
    }

    #[test]
    fn candidate_preference_and_shared_volumes() {
        let file = File { name: b"movie.mkv".to_vec(), ..File::default() };
        let first = Volume { filename: b"a.part01.rar".to_vec(), has_next_volume: true,
            multi_volume: true, new_naming: true, files: vec![file.clone()], ..Volume::default() };
        let second = Volume { filename: b"a.part02.rar".to_vec(), volume_no: 1,
            has_next_volume: false, ..first.clone() };
        let decoy = Volume { filename: b"b.part02.rar".to_vec(), ..second.clone() };
        let twin = Volume { filename: b"z.part01.rar".to_vec(), ..first.clone() };
        assert_eq!(make_sets(&[first, decoy, second, twin]), vec![vec![0, 2], vec![3, 1]]);
    }

    #[test]
    fn rar_renamer_full_suite_port_from_cpp() {
        struct BytesSource(Vec<u8>);
        impl crate::rarreader::Source for BytesSource {
            fn size(&mut self) -> i64 { self.0.len() as i64 }
            fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool {
                let Some(slice) = self.0.get(offset as usize..offset as usize + buf.len()) else { return false };
                buf.copy_from_slice(slice);
                true
            }
        }

        struct FsRarEnv<'a> {
            volumes: &'a [Volume],
            renamed_count: i32,
            errors: i32,
        }

        impl Env for FsRarEnv<'_> {
            fn file_exists(&mut self, path: &[u8]) -> bool {
                std::path::Path::new(std::str::from_utf8(path).unwrap()).is_file()
            }
            fn match_part(&mut self, name: &[u8]) -> Option<(usize, usize)> {
                // RegEx(".*.part([0-9]+)\\.rar$") with greedy ".*." before "part"
                let s = c(name);
                if !s.ends_with(b".rar") || s.len() < 9 {
                    return None;
                }
                let stem = &s[..s.len() - 4];
                let digit_start = stem.iter().rposition(|b| !b.is_ascii_digit()).map_or(0, |p| p + 1);
                if digit_start == stem.len() || digit_start < 5 {
                    return None;
                }
                if &stem[digit_start - 4..digit_start] != b"part" {
                    return None;
                }
                Some((digit_start, stem.len() - digit_start))
            }
            fn rename(&mut self, index: usize, dest: &[u8]) {
                let src_path = std::path::Path::new(std::str::from_utf8(&self.volumes[index].filename).unwrap());
                let dst_path = std::path::Path::new(std::str::from_utf8(dest).unwrap());
                if std::fs::rename(src_path, dst_path).is_ok() {
                    self.renamed_count += 1;
                } else {
                    self.errors += 1;
                }
            }
        }

        fn build_dir_list(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            out.push(dir.to_path_buf());
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        build_dir_list(&p, out);
                    }
                }
            }
        }

        fn execute_rar_renamer(dest_dir: &std::path::Path, password: Option<&[u8]>) -> (i32, i32) {
            let mut dirs = Vec::new();
            build_dir_list(dest_dir, &mut dirs);
            let mut total_renamed = 0;
            let mut total_errors = 0;

            for dir in dirs {
                let mut volumes = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_dir() {
                            continue;
                        }
                        let Ok(bytes) = std::fs::read(&p) else { continue };
                        let mut source = BytesSource(bytes);
                        let mut rvol = crate::rarreader::Volume {
                            has_password: password.is_some(),
                            ..crate::rarreader::Volume::default()
                        };
                        let mut cipher = crate::streamrepair::NativeRarCipher::new(password.unwrap_or(b""));
                        let mut rfiles: Vec<crate::rarreader::RarFile> = Vec::new();
                        if crate::rarreader::read_from(&mut rvol, &mut source, &mut cipher, &mut rfiles) {
                            let files = rfiles
                                .into_iter()
                                .map(|f| File {
                                    name: f.filename,
                                    size: f.size,
                                    split_before: f.split_before,
                                    split_after: f.split_after,
                                })
                                .collect();
                            volumes.push(Volume {
                                filename: p.to_string_lossy().as_bytes().to_vec(),
                                version: rvol.version,
                                volume_no: rvol.volume_no,
                                multi_volume: rvol.multi_volume,
                                has_next_volume: rvol.has_next_volume,
                                new_naming: rvol.new_naming,
                                files,
                            });
                        }
                    }
                }
                if !volumes.is_empty() {
                    let mut env = FsRarEnv {
                        volumes: &volumes,
                        renamed_count: 0,
                        errors: 0,
                    };
                    let dir_bytes = dir.to_string_lossy().as_bytes().to_vec();
                    rename_files(&dir_bytes, &volumes, &mut env);
                    total_renamed += env.renamed_count;
                    total_errors += env.errors;
                }
            }
            (total_renamed, total_errors)
        }

        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let testdata_dir = manifest_dir.parent().unwrap().join("tests/testdata/rarrenamer");

        let run_case = |name: &str, pwd: Option<&[u8]>, setup: &dyn Fn(&std::path::Path), verify: &dyn Fn(&std::path::Path, i32, i32)| {
            let work_dir = std::env::temp_dir().join(format!("nzbget_rs_rarrenamer_{}_{}", std::process::id(), name));
            let _ = std::fs::remove_dir_all(&work_dir);
            std::fs::create_dir_all(&work_dir).unwrap();
            for entry in std::fs::read_dir(&testdata_dir).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), work_dir.join(entry.file_name())).unwrap();
            }
            setup(&work_dir);
            let (renamed, errors) = execute_rar_renamer(&work_dir, pwd);
            verify(&work_dir, renamed, errors);
            let _ = std::fs::remove_dir_all(&work_dir);
        };

        // 1. RarRenameNotNeededTest
        run_case("RarRenameNotNeededTest", None, &|_| {}, &|_, renamed, _| assert_eq!(renamed, 0));

        // 2. RarRenameNotNeeded2Test
        run_case(
            "RarRenameNotNeeded2Test",
            None,
            &|w| {
                std::fs::copy(w.join("testfile5.part02.rar"), w.join("12348")).unwrap();
                std::fs::copy(w.join("testfile3oldnam.r00"), w.join("testfile3oldnamB.r00")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 0),
        );

        // 3. RarRenameSlowDataTest
        run_case(
            "RarRenameSlowDataTest",
            None,
            &|w| {
                std::fs::copy(w.join("testfile3.part01.rar"), w.join("12348")).unwrap();
                std::fs::copy(w.join("testfile3.part02.rar"), w.join("12342")).unwrap();
                std::fs::copy(w.join("testfile3.part03.rar"), w.join("12346")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 4. RenameRar5Test
        run_case(
            "RenameRar5Test",
            None,
            &|w| {
                std::fs::copy(w.join("testfile5.part01.rar"), w.join("12348")).unwrap();
                std::fs::copy(w.join("testfile5.part02.rar"), w.join("12342")).unwrap();
                std::fs::copy(w.join("testfile5.part03.rar"), w.join("12346")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 5. MissingPartsTest
        run_case(
            "MissingPartsTest",
            None,
            &|w| {
                std::fs::copy(w.join("testfile5.part01.rar"), w.join("12348")).unwrap();
                std::fs::copy(w.join("testfile5.part02.rar"), w.join("12343")).unwrap();
                std::fs::remove_file(w.join("testfile5.part03.rar")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 0),
        );

        // 6. RenameRar3BadNamingTest
        run_case(
            "RenameRar3BadNamingTest",
            None,
            &|w| {
                std::fs::rename(w.join("testfile3.part01.rar"), w.join("testfile3.part04.rar")).unwrap();
                std::fs::rename(w.join("testfile3.part02.rar"), w.join("testfile3.part01.rar")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 7. Rar3BadNaming2
        run_case(
            "Rar3BadNaming2",
            None,
            &|w| {
                std::fs::rename(w.join("testfile3.part02.rar"), w.join("testfile3.part2.rar")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 8. RenameRar3BadNaming3Test
        run_case(
            "RenameRar3BadNaming3Test",
            None,
            &|w| {
                std::fs::rename(w.join("testfile3.part02.rar"), w.join("testfile3-1.part02.rar")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 9. RenameRar3BadNaming4Test
        run_case(
            "RenameRar3BadNaming4Test",
            None,
            &|w| {
                std::fs::rename(w.join("testfile3.part02.rar"), w.join("testfil-3.part02.rar")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 10. RenameRar3BadNaming5Test
        run_case(
            "RenameRar3BadNaming5Test",
            None,
            &|w| {
                std::fs::rename(w.join("testfile3oldnam.rar"), w.join("testfile3oldnamA.rar")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 1),
        );

        // 11. RenameRar3BadNaming6Test
        run_case(
            "RenameRar3BadNaming6Test",
            None,
            &|w| {
                std::fs::rename(w.join("testfile3oldnam.rar"), w.join("testfile3oldnamA.rar")).unwrap();
                std::fs::rename(w.join("testfile3oldnam.r00"), w.join("testfile3oldnamB.r00")).unwrap();
                std::fs::rename(w.join("testfile3oldnam.r01"), w.join("testfile3oldnamA.r01")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 12. RenameTwoSetsTest
        run_case(
            "RenameTwoSetsTest",
            None,
            &|w| {
                std::fs::rename(w.join("testfile3.part01.rar"), w.join("12345")).unwrap();
                std::fs::rename(w.join("testfile3.part02.rar"), w.join("12342")).unwrap();
                std::fs::rename(w.join("testfile3.part03.rar"), w.join("12346")).unwrap();
                std::fs::rename(w.join("testfile5.part01.rar"), w.join("12348")).unwrap();
                std::fs::rename(w.join("testfile5.part02.rar"), w.join("12343")).unwrap();
                std::fs::rename(w.join("testfile5.part03.rar"), w.join("12344")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 6),
        );

        // 13. RenameDuplicateTest
        run_case(
            "RenameDuplicateTest",
            None,
            &|w| {
                std::fs::rename(w.join("testfile3.part01.rar"), w.join("12345")).unwrap();
                std::fs::rename(w.join("testfile3.part02.rar"), w.join("12342")).unwrap();
                std::fs::rename(w.join("testfile3.part03.rar"), w.join("12346")).unwrap();
                std::fs::rename(w.join("testfile5.part01.rar"), w.join("testfile3.dat.part0001.rar")).unwrap();
                std::fs::remove_file(w.join("testfile5.part02.rar")).unwrap();
                std::fs::remove_file(w.join("testfile5.part03.rar")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 14. RenameRar5EncryptedTest
        run_case(
            "RenameRar5EncryptedTest",
            Some(b"123"),
            &|w| {
                std::fs::rename(w.join("testfile5encnam.part01.rar"), w.join("12348")).unwrap();
                std::fs::rename(w.join("testfile5encnam.part02.rar"), w.join("12343")).unwrap();
                std::fs::rename(w.join("testfile5encnam.part03.rar"), w.join("12344")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 15. RenameRar3EncryptedTest
        run_case(
            "RenameRar3EncryptedTest",
            Some(b"123"),
            &|w| {
                std::fs::rename(w.join("testfile3encnam.part01.rar"), w.join("12348")).unwrap();
                std::fs::rename(w.join("testfile3encnam.part02.rar"), w.join("12343")).unwrap();
                std::fs::rename(w.join("testfile3encnam.part03.rar"), w.join("12344")).unwrap();
            },
            &|_, renamed, _| assert_eq!(renamed, 3),
        );

        // 16. RarRenameSubfolderTest
        run_case(
            "RarRenameSubfolderTest",
            None,
            &|w| {
                let sub = w.join("sub");
                std::fs::create_dir(&sub).unwrap();
                let names = ["12348", "12342", "12346"];
                for (i, name) in names.iter().enumerate() {
                    std::fs::copy(w.join(format!("testfile3.part0{}.rar", i + 1)), w.join(name)).unwrap();
                    std::fs::copy(w.join(format!("testfile5.part0{}.rar", i + 1)), sub.join(name)).unwrap();
                }
                for orig in ["testfile3.part01.rar", "testfile3.part02.rar", "testfile3.part03.rar"] {
                    std::fs::remove_file(w.join(orig)).unwrap();
                }
            },
            &|w, renamed, errors| {
                let sub = w.join("sub");
                assert_eq!(renamed, 6);
                assert!(w.join("testfile3.part0001.rar").exists());
                assert!(sub.join("testfile5.part0001.rar").exists());
                assert!(!sub.join("testfile3.part0001.rar").exists());
                assert_eq!(errors, 0);
            },
        );
    }
}
