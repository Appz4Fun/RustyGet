//! Par-rename (ParRenamer::Execute, daemon/postprocess/ParRenamer.cpp) runs
//! through `par_rs::renamer::ParRenamer`: par2 files found by their packet
//! header and renamed to "<setid>.volNNN+01.PAR2" when their names don't tell
//! their sets, the par2 files loaded, every other file renamed to the name the
//! par2 files give the MD5 of its first 16 KiB, a damaged par2 file renamed to
//! .bad once, missing files reported. This module maps par-rs's events onto
//! the job's callbacks (progress, messages, parred and renamed files) and
//! keeps the par2 listing helpers the direct renamer and post-processing use.

/// Message kind passed to the job's message callback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Info,
    Warning,
    Error,
}

/// An entry of a loaded par2 file's source file map, in the map's order.
pub enum ParEntry {
    /// no source file or no description packet
    Damaged,
    /// the local file name and the printed 16 KiB hash
    File { name: Vec<u8>, hash: Vec<u8> },
}

/// What a par-rename run reports back to the job.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub renamed_count: i32,
    pub has_missed_files: bool,
    pub has_damaged_par_files: bool,
}

/// NUL-terminated copy of `s` cut at its first NUL (the C++ C-string view).
fn c(s: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = s.iter().take_while(|&&b| b != 0).copied().collect();
    v.push(0);
    v
}

fn base_name(path: &[u8]) -> &[u8] {
    &path[crate::paths::base_file_name(path)..]
}

/// ParRenamer::IsSplittedFragment: the disk file is "<correct name>.NNN"
/// with NNN (all digits) at most 1 (.000 or .001), as par-rs decides it.
pub fn is_splitted_fragment(filename: &[u8], correct_name: &[u8]) -> bool {
    let base = c(base_name(filename));
    let correct = c(correct_name);
    par_rs::renamer::is_split_fragment(&base[..base.len() - 1], &correct[..correct.len() - 1])
}

/// A PAR2 name as a local file name (par-rs: `\\` as `/`, control bytes, a
/// leading `/` and `../` URL-encoded, UTF-8 kept, anything else read as Latin-1).
pub fn translate_filename_from_par2_to_local(par2_name: &[u8]) -> Vec<u8> {
    par_rs::packet::translate_filename_from_par2_to_local(par2_name).into_bytes()
}

/// The `ParEntry` items of a par2 set read by par-rs, in libpar2's order: a
/// file id with a description gives its local name and printed 16 KiB hash, a
/// file id with only slice checksums is damaged. None when no packet of the
/// set could be read.
fn entries_of_set(set: par_rs::packet::PacketSet) -> Option<Vec<ParEntry>> {
    if set.main.is_none()
        && set.creator.is_none()
        && set.file_descs.is_empty()
        && set.ifscs.is_empty()
        && set.recovery.is_empty()
    {
        return None;
    }
    let mut ids: Vec<par_rs::packet::FileId> = set.file_descs.keys().copied().collect();
    ids.extend(
        set.ifscs
            .keys()
            .filter(|id| !set.file_descs.contains_key(id)),
    );
    ids.sort();
    Some(
        ids.into_iter()
            .map(|id| match set.file_descs.get(&id) {
                Some(desc) => ParEntry::File {
                    name: desc.local_name().into_bytes(),
                    hash: crate::renameanalyzer::print(&desc.hash_16k)[..32].to_vec(),
                },
                None => ParEntry::Damaged,
            })
            .collect(),
    )
}

/// Parses PAR2 packets (`FileDesc` and `IFSC`) from `bytes` into `ParEntry` items
/// ordered identically to `Par2::Par2Repairer::sourcefilemap`.
pub fn parse_par2_packets(bytes: &[u8]) -> Option<Vec<ParEntry>> {
    use par_rs::packet::{load_par2_bytes, ScanOptions};
    entries_of_set(load_par2_bytes(bytes, ScanOptions::METADATA).ok()?)
}

/// Loads `ParEntry` items from a `.par2` file on disk (`DirectParLoader::LoadParFile`)
/// by streaming packets with a bounded 1 MiB buffer.
pub fn load_par_entries_from_file(par_filename: &std::path::Path) -> Option<Vec<ParEntry>> {
    let file = std::fs::File::open(par_filename).ok()?;
    let meta = file.metadata().ok()?;
    if meta.is_dir() {
        return None;
    }
    drop(file);
    use par_rs::packet::{load_par2_file, ScanOptions};
    entries_of_set(load_par2_file(par_filename, ScanOptions::METADATA).ok()?)
}

/// `ParRenamer::ListParredFiles`: returns base names of all files listed in `par_filename`'s
/// description packets, or `None` if `par_filename` cannot be read or exceeds `max_index_par_size`.
pub fn list_parred_files(
    par_filename: &std::path::Path,
    max_index_par_size: i64,
) -> Option<Vec<Vec<u8>>> {
    let file = std::fs::File::open(par_filename).ok()?;
    let meta = file.metadata().ok()?;
    if meta.is_dir() || (meta.len() as i64) > max_index_par_size {
        return None;
    }
    if meta.len() == 0 {
        return Some(Vec::new());
    }
    drop(file);
    use par_rs::packet::{load_par2_file, ScanOptions};
    let entries = entries_of_set(load_par2_file(par_filename, ScanOptions::METADATA).ok()?)?;
    let mut out = Vec::new();
    for entry in entries {
        if let ParEntry::File { name, .. } = entry {
            let name_c = c(&name);
            let trimmed = &name_c[..name_c.len() - 1];
            out.push(base_name(trimmed).to_vec());
        }
    }
    Some(out)
}

#[cfg(unix)]
fn bytes_to_path_buf(bytes: &[u8]) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path_buf(bytes: &[u8]) -> std::path::PathBuf {
    std::path::PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(unix)]
fn path_to_bytes(path: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_to_bytes(path: &std::path::Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

fn weakly_canonical(path: &std::path::Path) -> std::path::PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return canon;
    }
    let mut tail = Vec::new();
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if let Some(name) = cur.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(mut canon) = std::fs::canonicalize(parent) {
            for comp in tail.into_iter().rev() {
                canon.push(comp);
            }
            return canon;
        }
        cur = parent;
    }
    path.to_path_buf()
}

/// `ParRenamer::MakeRelativeName`: weakly-canonical path relative to `dest_dir`,
/// falling back to the filename if outside `dest_dir`.
pub fn make_relative_name(dest_dir: &[u8], full_filename: &[u8]) -> Vec<u8> {
    let canon_dest = weakly_canonical(&bytes_to_path_buf(dest_dir));
    let canon_file = weakly_canonical(&bytes_to_path_buf(full_filename));
    if let Ok(rel) = canon_file.strip_prefix(&canon_dest) {
        if !rel.as_os_str().is_empty() {
            return path_to_bytes(rel);
        }
    }
    canon_file
        .file_name()
        .map(|n| path_to_bytes(std::path::Path::new(n)))
        .unwrap_or_else(|| base_name(full_filename).to_vec())
}

/// Runs par-rename on `dest_dir` and its subdirectories through
/// `par_rs::renamer::ParRenamer`, reporting progress, messages, every file
/// the par2 sets describe and every rename to the job's callbacks.
#[allow(clippy::too_many_arguments)]
pub fn execute_all(
    info_name: &[u8],
    dest_dir: &[u8],
    ignore_ext: &[u8],
    detect_missing: bool,
    is_stopped: &mut dyn FnMut() -> bool,
    on_progress: &mut dyn FnMut(&[u8], i32),
    on_message: &mut dyn FnMut(Kind, &[u8]),
    on_register_parred: &mut dyn FnMut(&[u8]),
    on_register_renamed: &mut dyn FnMut(&[u8], &[u8]),
) -> State {
    execute_all_known(
        info_name,
        dest_dir,
        ignore_ext,
        detect_missing,
        &[],
        is_stopped,
        on_progress,
        on_message,
        on_register_parred,
        on_register_renamed,
    )
}

/// A queue file's 16k hash as the download stores it (`FileStr::Hash16k`, the article
/// analyzer's output): printed the way nzbget prints an MD5, its bytes last to first in
/// upper-case hex (`renameanalyzer::print`). Returns the digest in its own byte order, the
/// order par2 file descriptions and `par_rs` use. Before, the printed text was read as if it
/// were in digest order, so every known hash was reversed: par-rename reported every file
/// it had been told about as missing and asked for a par-check (6790 audit, finding 3).
pub fn parse_printed_hash16k(printed: &[u8]) -> Option<[u8; 16]> {
    let mut hash = parse_hash16k(printed)?;
    hash.reverse();
    Some(hash)
}

/// 32 hex digits read as a 16-byte digest in the order written. None for anything else.
pub fn parse_hash16k(hex: &[u8]) -> Option<[u8; 16]> {
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, pair) in hex.chunks(2).enumerate() {
        let digit = |b: u8| (b as char).to_digit(16);
        out[i] = u8::try_from(digit(pair[0])? * 16 + digit(pair[1])?).ok()?;
    }
    Some(out)
}

/// [`execute_all`] with the first-16-KiB MD5s the download already computed (`known`: file
/// name in `dest_dir`, hash), handed to par-rs (`ParRenamer::set_known_hash16k`) so it
/// identifies those files without reading them. The key is `dest_dir` joined with the name,
/// exactly the path the renamer lists; a name it does not list is simply read as before.
#[allow(clippy::too_many_arguments)]
pub fn execute_all_known(
    info_name: &[u8],
    dest_dir: &[u8],
    ignore_ext: &[u8],
    detect_missing: bool,
    known: &[(Vec<u8>, [u8; 16])],
    is_stopped: &mut dyn FnMut() -> bool,
    on_progress: &mut dyn FnMut(&[u8], i32),
    on_message: &mut dyn FnMut(Kind, &[u8]),
    on_register_parred: &mut dyn FnMut(&[u8]),
    on_register_renamed: &mut dyn FnMut(&[u8], &[u8]),
) -> State {
    use par_rs::renamer::{ParRenamer, RenamerEvent, RenamerOptions};

    on_progress(&[b"Checking renamed files for ", info_name].concat(), 0);

    let ignore_list = ignore_ext.to_vec();
    let options = RenamerOptions {
        detect_missing,
        is_ignored: Some(std::sync::Arc::new(move |name: &str| {
            let fold = crate::wildmask::Lower::Ascii;
            crate::util::match_file_ext(name.as_bytes(), &ignore_list, b",;", &fold, &fold)
        })),
        ..RenamerOptions::default()
    };

    let mut checking: Vec<u8> = Vec::new();
    let mut events = |ev: RenamerEvent| match ev {
        RenamerEvent::Info(t) => on_message(Kind::Info, t.as_bytes()),
        RenamerEvent::Warning(t) => on_message(Kind::Warning, t.as_bytes()),
        RenamerEvent::Error(t) => on_message(Kind::Error, t.as_bytes()),
        RenamerEvent::Checking { path } => {
            checking = path
                .file_name()
                .map(|n| path_to_bytes(std::path::Path::new(n)))
                .unwrap_or_default();
        }
        RenamerEvent::Progress(cur, total) => {
            let stage = cur
                .saturating_mul(1000)
                .checked_div(total)
                .map_or(1000, |s| s.min(1000) as i32);
            on_progress(&[&b"Checking file "[..], &checking].concat(), stage);
        }
        RenamerEvent::Described { name } => on_register_parred(name.as_bytes()),
        RenamerEvent::Renamed { from, to } => {
            let old_rel = make_relative_name(dest_dir, &path_to_bytes(&from));
            let new_rel = make_relative_name(dest_dir, &path_to_bytes(&to));
            on_register_renamed(&old_rel, &new_rel);
        }
        _ => {}
    };

    let mut renamer = ParRenamer::new(bytes_to_path_buf(dest_dir), options);
    let root = bytes_to_path_buf(dest_dir);
    for (name, hash) in known {
        renamer.set_known_hash16k(root.join(bytes_to_path_buf(name)), *hash);
    }
    match renamer.run_with_stop(is_stopped, &mut events) {
        Ok(report) => State {
            renamed_count: i32::try_from(report.renamed_count()).unwrap_or(i32::MAX),
            has_missed_files: report.has_missed_files,
            has_damaged_par_files: report.has_damaged_par_files,
        },
        Err(_) => State::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use md5::{Digest, Md5};

    /// The analyzer prints a digest backwards; read back, it is the digest a par2 file
    /// description holds.
    #[test]
    fn printed_hash16k_reads_back_in_digest_order() {
        let bytes: Vec<u8> = (0..20_000u32).map(|i| (i * 7 + 3) as u8).collect();
        let digest: [u8; 16] = Md5::digest(&bytes[..16384]).into();
        let mut analyzer = crate::renameanalyzer::Analyzer::default();
        analyzer.append(&bytes[..9000]);
        analyzer.append(&bytes[9000..]);
        let printed = analyzer.finish();
        assert_eq!(parse_printed_hash16k(&printed[..32]), Some(digest));
        assert_ne!(parse_hash16k(&printed[..32]), Some(digest));
        assert_eq!(parse_printed_hash16k(b"not a hash"), None);
    }

    #[test]
    fn splitted_fragments() {
        assert!(is_splitted_fragment(b"/d/movie.mkv.001", b"movie.mkv"));
        assert!(is_splitted_fragment(b"/d/MOVIE.MKV.000", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/movie.mkv.", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/movie.mkv.002", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/movie.mkv.001x", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/movie.mkv", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/other.001", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/a.99999999999999999999999", b"a"));
    }

    #[test]
    fn par_renamer_full_suite_port_from_cpp() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let testdata_root = manifest_dir.parent().unwrap().join("tests/testdata");
        let parchecker_dir = testdata_root.join("parchecker");
        let parchecker_utf8_dir = testdata_root.join("parcheckerUtf8");

        let copy_dir = |src: &std::path::Path, dst: &std::path::Path| {
            let _ = std::fs::remove_dir_all(dst);
            std::fs::create_dir_all(dst).unwrap();
            for entry in std::fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), dst.join(entry.file_name())).unwrap();
            }
        };

        let run_case = |name: &str,
                        src_dir: &std::path::Path,
                        detect_missing: bool,
                        setup: &dyn Fn(&std::path::Path)|
         -> State {
            let work_dir = std::env::temp_dir().join(format!(
                "nzbget_rs_parrenamer_{}_{}",
                std::process::id(),
                name
            ));
            copy_dir(src_dir, &work_dir);
            setup(&work_dir);
            let dest_bytes = work_dir.to_string_lossy().as_bytes().to_vec();
            let state = execute_all(
                b"test",
                &dest_bytes,
                b"",
                detect_missing,
                &mut || false,
                &mut |_, _| {},
                &mut |_, _| {},
                &mut |_| {},
                &mut |_, _| {},
            );
            let _ = std::fs::remove_dir_all(&work_dir);
            state
        };

        // 1. RenameNotNeededTest
        let st = run_case("RenameNotNeededTest", &parchecker_dir, false, &|_| {});
        assert_eq!(st.renamed_count, 0);

        // 2. RenameSuccessfulTest
        let st = run_case("RenameSuccessfulTest", &parchecker_dir, false, &|w| {
            std::fs::rename(w.join("testfile.dat"), w.join("123456")).unwrap();
        });
        assert_eq!(st.renamed_count, 1);
        assert!(!st.has_missed_files);

        // 3. DetectingMissingTest
        let st = run_case("DetectingMissingTest", &parchecker_dir, true, &|w| {
            std::fs::rename(w.join("testfile.dat"), w.join("123456")).unwrap();
            std::fs::remove_file(w.join("testfile.nfo")).unwrap();
        });
        assert_eq!(st.renamed_count, 1);
        assert!(st.has_missed_files);

        // 4. RenameDupeParTest
        let st = run_case("RenameDupeParTest", &parchecker_dir, true, &|w| {
            std::fs::rename(w.join("testfile.dat"), w.join("123456")).unwrap();
            std::fs::rename(w.join("testfile.vol00+1.PAR2"), w.join("testfil2.par2")).unwrap();
        });
        assert_eq!(st.renamed_count, 5);
        assert!(!st.has_missed_files);

        // 5. NoParExtensionTest
        let st = run_case("NoParExtensionTest", &parchecker_dir, true, &|w| {
            std::fs::rename(w.join("testfile.par2"), w.join("testfile")).unwrap();
        });
        assert_eq!(st.renamed_count, 4);
        assert!(!st.has_missed_files);

        // 6. Utf8Par2Test
        let st = run_case("Utf8Par2Test", &parchecker_utf8_dir, false, &|_| {});
        assert_eq!(st.renamed_count, 1);
        assert!(!st.has_missed_files);

        // 7. list_parred_files test
        let listed =
            list_parred_files(&parchecker_dir.join("testfile.par2"), 16 * 1024 * 1024).unwrap();
        assert_eq!(listed.len(), 2);
    }

    /// A normal download: every data file came down whole, so its first-16-KiB MD5 is known
    /// from its first article. par-rename identifies the files from those hashes without
    /// their 16 KiB read: here they cannot be read at all (mode 000), and still every one is
    /// renamed. Without the hashes the same run fails to read them.
    #[cfg(unix)]
    #[test]
    fn par_rename_reads_no_data_file_whose_hash16k_is_known() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return; // root reads mode-000 files: the check below would prove nothing
        }
        let src =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/testdata/parchecker");
        let run = |tag: &str, with_hashes: bool| -> (State, Vec<Vec<u8>>) {
            let work = std::env::temp_dir()
                .join(format!("rustyget-known16k-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&work);
            std::fs::create_dir_all(&work).unwrap();
            for entry in std::fs::read_dir(&src).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), work.join(entry.file_name())).unwrap();
            }
            // Obfuscated names, as posted, and the hashes the download computed.
            let mut known = Vec::new();
            for (real, posted) in [("testfile.dat", "a1b2c3"), ("testfile.nfo", "d4e5f6")] {
                let bytes = std::fs::read(work.join(real)).unwrap();
                let hash: [u8; 16] = Md5::digest(&bytes[..bytes.len().min(16384)]).into();
                let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
                assert_eq!(parse_hash16k(hex.as_bytes()), Some(hash));
                std::fs::rename(work.join(real), work.join(posted)).unwrap();
                std::fs::set_permissions(work.join(posted), std::fs::Permissions::from_mode(0))
                    .unwrap();
                known.push((posted.as_bytes().to_vec(), hash));
            }
            let mut errors = Vec::new();
            let state = execute_all_known(
                b"test",
                work.to_string_lossy().as_bytes(),
                b"",
                false,
                if with_hashes { &known } else { &[] },
                &mut || false,
                &mut |_, _| {},
                &mut |kind, text| {
                    if matches!(kind, Kind::Error) {
                        errors.push(text.to_vec());
                    }
                },
                &mut |_| {},
                &mut |_, _| {},
            );
            for name in ["a1b2c3", "d4e5f6", "testfile.dat", "testfile.nfo"] {
                let _ = std::fs::set_permissions(
                    work.join(name),
                    std::fs::Permissions::from_mode(0o644),
                );
            }
            let _ = std::fs::remove_dir_all(&work);
            (state, errors)
        };
        // par-rs still opens every file once in its par2 pass to look at the first packet
        // header (64 bytes; "Could not open file" here). The 16 KiB identity read ("Could
        // not read file") is what the known hashes remove.
        let reads16k = |errors: &[Vec<u8>]| {
            errors
                .iter()
                .filter(|e| e.starts_with(b"Could not read file"))
                .count()
        };
        let (state, errors) = run("known", true);
        assert_eq!(
            state.renamed_count, 2,
            "both files identified from their known hashes"
        );
        assert_eq!(reads16k(&errors), 0, "no 16 KiB read: {errors:?}");
        let (state, errors) = run("unknown", false);
        assert_eq!(state.renamed_count, 0);
        assert_eq!(
            reads16k(&errors),
            2,
            "without the hashes the renamer reads each file's first 16 KiB"
        );
    }

    #[test]
    fn par2_names_decode_utf8_first_then_latin1() {
        fn desc_packet(id: u8, name: &[u8]) -> Vec<u8> {
            let mut body = [7u8; 16].to_vec(); // set id
            body.extend_from_slice(b"PAR 2.0\0FileDesc");
            body.extend_from_slice(&[id; 16]);
            body.extend_from_slice(&[9; 32]); // full and 16 KiB hashes, equal
            body.extend_from_slice(&100u64.to_le_bytes());
            body.extend_from_slice(name);
            body.resize(body.len().div_ceil(4) * 4, 0);
            let mut out = b"PAR2\0PKT".to_vec();
            out.extend_from_slice(&((body.len() + 32) as u64).to_le_bytes());
            out.extend_from_slice(&Md5::digest(&body));
            out.extend_from_slice(&body);
            out
        }
        let mut data = desc_packet(1, "caf\u{e9}.mkv".as_bytes());
        data.extend(desc_packet(2, b"na\xefve.nfo"));
        let entries = parse_par2_packets(&data).unwrap();
        let names: Vec<Vec<u8>> = entries
            .into_iter()
            .map(|e| match e {
                ParEntry::File { name, .. } => name,
                ParEntry::Damaged => Vec::new(),
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "caf\u{e9}.mkv".as_bytes().to_vec(),
                "na\u{ef}ve.nfo".as_bytes().to_vec()
            ]
        );
    }

    #[test]
    fn parse_par2_packets_rejects_overflowing_packet_length() {
        let mut buf = vec![0u8; 160];
        // Header at offset 64 with length 0xfffffffffffffffc that would wrap offset + length to 60
        buf[64..72].copy_from_slice(&par_rs::packet::MAGIC);
        buf[72..80].copy_from_slice(&0xffff_ffff_ffff_fffcu64.to_le_bytes());
        assert!(parse_par2_packets(&buf).is_none());
    }
}
