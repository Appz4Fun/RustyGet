//! ParRenamer::Execute (daemon/postprocess/ParRenamer.cpp): par-rename - the
//! destination directory and its subdirectories walked, par2 files found by
//! their packet header and renamed to "<setid>.volNNN+01.PAR2" when their
//! names don't tell their sets, the par2 files loaded, and every other file
//! renamed to the name the par2 files give the MD5 of its first 16 KiB (the
//! first par2 entry with that hash; nothing else is compared), as the C++
//! did it: the same order of files, checks, messages and renames, paths cut
//! as BString<1024> cut them, a damaged par2 file listed once and renamed to
//! .bad, missing files reported. Loading par2 packets, listing a directory
//! (a snapshot), file checks and reads, moving a file and the messages stay
//! with the caller (`Env`).

use md5::{Digest, Md5};

/// A C++ exception in a callback: the run ends, the caller rethrows it.
#[derive(Debug)]
pub struct Abort;

pub type R<T> = Result<T, Abort>;

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

/// A file read from its start.
pub enum Read {
    OpenFailed,
    ReadFailed,
    Bytes(usize),
}

pub trait Env {
    /// Read the destination after the initial UpdateProgress, which may change it.
    fn destination(&mut self, initial: &[u8]) -> R<Vec<u8>>;
    /// Publish changed members before another callback can observe them.
    fn state_changed(&mut self, state: &mut State);
    /// Preserve the C++ directory list across Execute calls, including aborted runs.
    fn previous_dirs(&mut self) -> R<Vec<Vec<u8>>>;
    fn remember_dir(&mut self, dir: &[u8]) -> R<()>;
    fn stopped(&mut self) -> R<bool>;
    /// the progress label and stage set, then UpdateProgress
    fn progress(&mut self, label: &[u8], stage: i32) -> R<()>;
    fn message(&mut self, kind: Kind, text: &[u8]) -> R<()>;
    fn debug(&mut self, text: &[u8]) -> R<()>;
    /// DirBrowser's snapshot of a directory
    fn list_dir(&mut self, dir: &[u8]) -> R<Vec<Vec<u8>>>;
    fn dir_exists(&mut self, path: &[u8]) -> R<bool>;
    fn file_exists(&mut self, path: &[u8]) -> R<bool>;
    fn file_size(&mut self, path: &[u8]) -> R<i64>;
    /// DiskFile::Read of the start of a file into `buf`
    fn read(&mut self, path: &[u8], buf: &mut [u8]) -> R<Read>;
    /// LoadPacketsFromFile (None when it fails or the file is empty).
    fn load_par(&mut self, path: &[u8]) -> R<Option<usize>>;
    /// Translate one entry only after the loaded message and IsStopped check.
    fn par_entry(&mut self) -> R<ParEntry>;
    fn close_par(&mut self);
    /// ParParser::FindMainPars
    fn find_main_pars(&mut self, dir: &[u8]) -> R<Vec<Vec<u8>>>;
    fn register_parred(&mut self, name: &[u8]) -> R<()>;
    /// RenameFile without the count: true when the file was moved
    fn rename(&mut self, src: &[u8], dst: &[u8]) -> R<bool>;
    /// Util::MatchFileExt with ParIgnoreExt
    fn ignored_ext(&mut self, name: &[u8]) -> R<bool>;
}

/// The members the caller reads and sets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub file_count: i32,
    pub cur_file: i32,
    pub renamed_count: i32,
    pub has_missed_files: bool,
    pub has_damaged_par_files: bool,
    pub detect_missing: bool,
}

const BSTRING: usize = 1023;
const HEADER: usize = 64;
const MAGIC: &[u8; 8] = b"PAR2\0PKT";

/// BString<1024>'s cut.
fn bstring(mut s: Vec<u8>) -> Vec<u8> {
    s.truncate(BSTRING);
    s
}

/// BString<1024>("%s%c%s", dir, PATH_SEPARATOR, name).
fn join(dir: &[u8], sep: u8, name: &[u8]) -> Vec<u8> {
    bstring([dir, &[sep], name].concat())
}

fn c(s: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = s.iter().take_while(|&&b| b != 0).copied().collect();
    v.push(0);
    v
}

fn base_name(path: &[u8]) -> &[u8] {
    &path[crate::paths::base_file_name(path)..]
}

fn text(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

struct FileHash {
    filename: Vec<u8>,
    hash: Vec<u8>,
    exists: bool,
}

struct ParInfo {
    filename: Vec<u8>,
    set_id: Vec<u8>,
}

pub struct ParRenamer<'a, E: Env> {
    env: &'a mut E,
    state: &'a mut State,
    sep: u8,
    dest_dir: Vec<u8>,
    dir_list: Vec<Vec<u8>>,
    file_hashes: Vec<FileHash>,
    par_infos: Vec<ParInfo>,
    bad_pars: Vec<Vec<u8>>,
    loaded_pars: Vec<Vec<u8>>,
}

/// ParRenamer::Execute.
pub fn execute<E: Env>(
    env: &mut E,
    state: &mut State,
    info_name: &[u8],
    dest_dir: &[u8],
    sep: u8,
) -> R<()> {
    let mut r = ParRenamer {
        env,
        state,
        sep,
        dest_dir: dest_dir.to_vec(),
        dir_list: Vec::new(),
        file_hashes: Vec::new(),
        par_infos: Vec::new(),
        bad_pars: Vec::new(),
        loaded_pars: Vec::new(),
    };
    r.run(info_name)
}

impl<E: Env> ParRenamer<'_, E> {
    fn run(&mut self, info_name: &[u8]) -> R<()> {
        self.env
            .progress(&text(&[b"Checking renamed files for ", info_name]), 0)?;
        let dest_dir = self.env.destination(&self.dest_dir)?;
        self.dir_list = self.env.previous_dirs()?;
        self.build_dir_list(&dest_dir)?;

        for dir in std::mem::take(&mut self.dir_list) {
            self.env.debug(&text(&[b"Checking ", &dir]))?;
            self.file_hashes.clear();
            self.par_infos.clear();
            self.bad_pars.clear();
            self.loaded_pars.clear();

            self.check_files(&dir, true)?;
            self.rename_par_files(&dir)?;

            self.load_main_par_files(&dir)?;
            if self.state.has_damaged_par_files {
                self.load_extra_par_files(&dir)?;
            }

            self.check_files(&dir, false)?;

            self.env.state_changed(self.state);
            if self.state.detect_missing {
                self.check_missing()?;
            }

            if self.state.renamed_count > 0 && !self.bad_pars.is_empty() {
                self.rename_bad_par_files()?;
            }
        }
        Ok(())
    }

    fn build_dir_list(&mut self, dir: &[u8]) -> R<()> {
        self.env.remember_dir(dir)?;
        self.dir_list.push(dir.to_vec());
        for name in self.env.list_dir(dir)? {
            if !self.env.stopped()? {
                let full = join(dir, self.sep, &name);
                if self.env.dir_exists(&full)? {
                    self.build_dir_list(&full)?;
                } else {
                    self.state.file_count = self.state.file_count.wrapping_add(1);
                    self.env.state_changed(self.state);
                }
            }
        }
        Ok(())
    }

    fn load_main_par_files(&mut self, dir: &[u8]) -> R<()> {
        for name in self.env.find_main_pars(dir)? {
            let full = join(dir, self.sep, &name);
            self.load_par_file(&full)?;
        }
        Ok(())
    }

    fn load_extra_par_files(&mut self, dir: &[u8]) -> R<()> {
        for name in self.env.list_dir(dir)? {
            let full = join(dir, self.sep, &name);
            if crate::parparser::parse_par_filename(&full, true).is_some()
                && !self.bad_pars.contains(&full)
                && !self.loaded_pars.contains(&full)
            {
                self.load_par_file(&full)?;
            }
        }
        Ok(())
    }

    fn load_par_file(&mut self, path: &[u8]) -> R<()> {
        let Some(count) = self.env.load_par(path)? else {
            self.env
                .message(Kind::Warning, &text(&[b"Could not load par2-file ", path]))?;
            self.state.has_damaged_par_files = true;
            self.env.state_changed(self.state);
            self.bad_pars.push(path.to_vec());
            return Ok(());
        };

        self.loaded_pars.push(path.to_vec());
        self.env.message(
            Kind::Info,
            &text(&[b"Loaded par2-file ", base_name(path), b" for par-rename"]),
        )?;

        // a par2-file missing several description packets is listed (and
        // later renamed to .bad) once
        let mut damaged = false;
        for _ in 0..count {
            if self.env.stopped()? {
                break;
            }
            match self.env.par_entry()? {
                ParEntry::Damaged => {
                    if !damaged {
                        self.env.message(
                            Kind::Warning,
                            &text(&[b"Damaged par2-file detected: ", base_name(path)]),
                        )?;
                        self.bad_pars.push(path.to_vec());
                        damaged = true;
                    }
                    self.state.has_damaged_par_files = true;
                    self.env.state_changed(self.state);
                }
                ParEntry::File { name, hash } => {
                    // the C++ compared and stored C strings
                    let name = c(&name);
                    let hash = c(&hash);
                    let (name, hash) = (&name[..name.len() - 1], &hash[..hash.len() - 1]);
                    if !self.file_hashes.iter().any(|f| f.hash == hash) {
                        self.file_hashes.push(FileHash {
                            filename: name.to_vec(),
                            hash: hash.to_vec(),
                            exists: false,
                        });
                        self.env.register_parred(name)?;
                    }
                }
            }
        }
        self.env.close_par();
        Ok(())
    }

    fn check_files(&mut self, dir: &[u8], check_pars: bool) -> R<()> {
        for name in self.env.list_dir(dir)? {
            if self.env.stopped()? {
                continue;
            }
            let full = join(dir, self.sep, &name);
            if self.env.dir_exists(&full)? {
                continue;
            }
            let stage = if self.state.file_count > 0 {
                self.state.cur_file.wrapping_mul(1000) / self.state.file_count / 2
            } else {
                1000
            };
            self.env
                .progress(&text(&[b"Checking file ", &name]), stage)?;
            self.state.cur_file = self.state.cur_file.wrapping_add(1);
            self.env.state_changed(self.state);

            if check_pars {
                self.check_par_file(&full)?;
            } else {
                self.check_regular_file(dir, &full)?;
            }
        }
        Ok(())
    }

    fn check_missing(&mut self) -> R<()> {
        for i in 0..self.file_hashes.len() {
            if self.file_hashes[i].exists {
                continue;
            }
            let name = self.file_hashes[i].filename.clone();
            if self.env.ignored_ext(&name)? {
                self.env.message(
                    Kind::Info,
                    &text(&[b"File ", &name, b" is missing, ignoring"]),
                )?;
            } else {
                self.env
                    .message(Kind::Info, &text(&[b"File ", &name, b" is missing"]))?;
                self.state.has_missed_files = true;
                self.env.state_changed(self.state);
            }
        }
        Ok(())
    }

    fn check_regular_file(&mut self, dir: &[u8], path: &[u8]) -> R<()> {
        self.env.debug(&text(&[b"Computing hash for ", path]))?;

        let mut buf = vec![0u8; 16 * 1024];
        let n = match self.env.read(path, &mut buf)? {
            Read::OpenFailed => {
                return self
                    .env
                    .message(Kind::Error, &text(&[b"Could not open file ", path]))
            }
            Read::ReadFailed => {
                return self
                    .env
                    .message(Kind::Error, &text(&[b"Could not read file ", path]))
            }
            Read::Bytes(n) => n.min(buf.len()),
        };
        let hash16k = crate::renameanalyzer::print(&Md5::digest(&buf[..n]));
        let hash16k = &hash16k[..32];

        self.env.debug(&text(&[
            b"file: ",
            base_name(path),
            b"; hash16k: ",
            hash16k,
        ]))?;

        let Some(i) = self.file_hashes.iter().position(|f| f.hash == hash16k) else {
            return Ok(());
        };
        let correct = self.file_hashes[i].filename.clone();
        self.env
            .debug(&text(&[b"Found correct filename: ", &correct]))?;
        self.file_hashes[i].exists = true;

        let dst = join(dir, self.sep, &correct);
        if !self.env.file_exists(&dst)? && !is_splitted_fragment(path, &correct) {
            self.rename_file(path, &dst)?;
        }
        Ok(())
    }

    fn check_par_file(&mut self, path: &[u8]) -> R<()> {
        self.env
            .debug(&text(&[b"Checking par2-header for ", path]))?;

        let mut header = [0u8; HEADER];
        match self.env.read(path, &mut header)? {
            Read::OpenFailed => {
                return self
                    .env
                    .message(Kind::Error, &text(&[b"Could not open file ", path]))
            }
            Read::ReadFailed => {
                return self
                    .env
                    .message(Kind::Error, &text(&[b"Could not read file ", path]))
            }
            Read::Bytes(_) => {}
        }

        let length = u64::from_le_bytes(header[8..16].try_into().unwrap());
        if &header[..8] != MAGIC
            || (HEADER as u64) > length
            || length & 3 != 0
            || self.env.file_size(path)? < length as u32 as i32 as i64
        {
            // not par2-file or damaged header, ignoring the file
            return Ok(());
        }

        let set_id: Vec<u8> = crate::renameanalyzer::print(&header[32..48])[..32]
            .iter()
            .map(u8::to_ascii_lowercase)
            .collect();

        self.env.debug(&text(&[
            b"Storing: ",
            base_name(path),
            b"; setid: ",
            &set_id,
        ]))?;
        self.par_infos.push(ParInfo {
            filename: path.to_vec(),
            set_id,
        });
        Ok(())
    }

    fn rename_par_files(&mut self, dir: &[u8]) -> R<()> {
        if self.need_rename_par_files() {
            for i in 0..self.par_infos.len() {
                let filename = self.par_infos[i].filename.clone();
                let set_id = self.par_infos[i].set_id.clone();
                self.rename_par_file(dir, &filename, &set_id)?;
            }
        }
        Ok(())
    }

    /// ParseParFilename's base length; 0 where the C++ read an uninitialized int.
    fn base_len(&self, name: &[u8]) -> usize {
        crate::parparser::parse_par_filename(name, true).map_or(0, |(len, _)| len)
    }

    fn need_rename_par_files(&self) -> bool {
        for (i, par1) in self.par_infos.iter().enumerate() {
            let base1 = &c(base_name(&par1.filename));
            let base1 = &base1[..base1.len() - 1];
            let Some(dot) = base1.iter().rposition(|&b| b == b'.') else {
                return true;
            };
            if !base1[dot..].eq_ignore_ascii_case(b".par2") {
                // file doesn't have "par2" extension
                return true;
            }
            let len1 = self.base_len(base1);
            for par2 in &self.par_infos[i + 1..] {
                if par1.set_id == par2.set_id {
                    let base2 = &c(base_name(&par2.filename));
                    let base2 = &base2[..base2.len() - 1];
                    let len2 = self.base_len(base2);
                    if len1 != len2 || !base1[..len1].eq_ignore_ascii_case(&base2[..len2]) {
                        // same setid but different base file names
                        return true;
                    }
                }
            }
        }
        false
    }

    fn rename_par_file(&mut self, dir: &[u8], filename: &[u8], set_id: &[u8]) -> R<()> {
        self.env.debug(&text(&[
            b"Renaming: ",
            base_name(filename),
            b"; setid: ",
            set_id,
        ]))?;
        // the first name, then the next one while the last one exists
        let mut num: i32 = 1;
        loop {
            let dest = bstring(
                [
                    dir,
                    &[self.sep],
                    set_id,
                    format!(".vol{num:03}+01.PAR2").as_bytes(),
                ]
                .concat(),
            );
            num = num.wrapping_add(1);
            if !self.env.file_exists(&dest)? {
                return self.rename_file(filename, &dest);
            }
        }
    }

    fn rename_file(&mut self, src: &[u8], dst: &[u8]) -> R<()> {
        if self.env.rename(src, dst)? {
            self.state.renamed_count = self.state.renamed_count.wrapping_add(1);
            self.env.state_changed(self.state);
        }
        Ok(())
    }

    fn rename_bad_par_files(&mut self) -> R<()> {
        for bad in self.bad_pars.clone() {
            let dest = bstring([&bad[..], b".bad"].concat());
            self.rename_file(&bad, &dest)?;
        }
        Ok(())
    }
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

struct FsParRenamerEnv<'a> {
    dest_dir: Vec<u8>,
    ignore_ext: Vec<u8>,
    dir_list: Vec<Vec<u8>>,
    loaded_entries: std::vec::IntoIter<ParEntry>,
    is_stopped: &'a mut dyn FnMut() -> bool,
    on_progress: &'a mut dyn FnMut(&[u8], i32),
    on_message: &'a mut dyn FnMut(Kind, &[u8]),
    on_register_parred: &'a mut dyn FnMut(&[u8]),
    on_register_renamed: &'a mut dyn FnMut(&[u8], &[u8]),
}

impl Env for FsParRenamerEnv<'_> {
    fn destination(&mut self, _initial: &[u8]) -> R<Vec<u8>> {
        Ok(self.dest_dir.clone())
    }
    fn state_changed(&mut self, _state: &mut State) {}
    fn previous_dirs(&mut self) -> R<Vec<Vec<u8>>> {
        Ok(self.dir_list.clone())
    }
    fn remember_dir(&mut self, dir: &[u8]) -> R<()> {
        self.dir_list.push(dir.to_vec());
        Ok(())
    }
    fn stopped(&mut self) -> R<bool> {
        Ok((self.is_stopped)())
    }
    fn progress(&mut self, label: &[u8], stage: i32) -> R<()> {
        (self.on_progress)(label, stage);
        Ok(())
    }
    fn message(&mut self, kind: Kind, text: &[u8]) -> R<()> {
        (self.on_message)(kind, text);
        Ok(())
    }
    fn debug(&mut self, _text: &[u8]) -> R<()> {
        Ok(())
    }
    fn list_dir(&mut self, dir: &[u8]) -> R<Vec<Vec<u8>>> {
        let path = bytes_to_path_buf(dir);
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&path) {
            for entry in entries.flatten() {
                out.push(path_to_bytes(std::path::Path::new(&entry.file_name())));
            }
        }
        Ok(out)
    }
    fn dir_exists(&mut self, path: &[u8]) -> R<bool> {
        Ok(bytes_to_path_buf(path).is_dir())
    }
    fn file_exists(&mut self, path: &[u8]) -> R<bool> {
        Ok(bytes_to_path_buf(path).is_file())
    }
    fn file_size(&mut self, path: &[u8]) -> R<i64> {
        Ok(std::fs::metadata(bytes_to_path_buf(path))
            .map(|m| m.len() as i64)
            .unwrap_or(-1))
    }
    fn read(&mut self, path: &[u8], buf: &mut [u8]) -> R<Read> {
        use std::io::Read as _;
        let Ok(mut f) = std::fs::File::open(bytes_to_path_buf(path)) else {
            return Ok(Read::OpenFailed);
        };
        let mut total = 0;
        while total < buf.len() {
            match f.read(&mut buf[total..]) {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Ok(Read::ReadFailed),
            }
        }
        Ok(Read::Bytes(total))
    }
    fn load_par(&mut self, path: &[u8]) -> R<Option<usize>> {
        let Some(entries) = load_par_entries_from_file(&bytes_to_path_buf(path)) else {
            return Ok(None);
        };
        let count = entries.len();
        self.loaded_entries = entries.into_iter();
        Ok(Some(count))
    }
    fn par_entry(&mut self) -> R<ParEntry> {
        Ok(self.loaded_entries.next().unwrap_or(ParEntry::Damaged))
    }
    fn close_par(&mut self) {
        self.loaded_entries = Vec::new().into_iter();
    }
    fn find_main_pars(&mut self, dir: &[u8]) -> R<Vec<Vec<u8>>> {
        let p = bytes_to_path_buf(dir);
        let mut list = Vec::new();
        crate::parparser::find_main_pars(&p, true, |name| {
            list.push(name.to_vec());
        });
        Ok(list)
    }
    fn register_parred(&mut self, name: &[u8]) -> R<()> {
        (self.on_register_parred)(name);
        Ok(())
    }
    fn rename(&mut self, src: &[u8], dst: &[u8]) -> R<bool> {
        let src_c_vec = c(src);
        let dst_c_vec = c(dst);
        let (Ok(src_c), Ok(dst_c)) = (
            std::ffi::CStr::from_bytes_with_nul(&src_c_vec),
            std::ffi::CStr::from_bytes_with_nul(&dst_c_vec),
        ) else {
            return Ok(false);
        };
        let (old_path, old_name) = crate::paths::split_path_and_filename(src_c.to_bytes());
        let (new_path, new_name) = crate::paths::split_path_and_filename(dst_c.to_bytes());
        (self.on_message)(
            Kind::Info,
            &text(&[b"Renaming ", old_name, b" to ", new_name]),
        );

        if old_path != new_path {
            let mut errmsg = Vec::new();
            if !crate::fsops::force_directories(new_path, &mut errmsg, &crate::fsops::posix_stat) {
                (self.on_message)(
                    Kind::Error,
                    &text(&[
                        b"Failed to rename file ",
                        old_name,
                        b" to ",
                        new_name,
                        b". Could not create the target directory \"",
                        new_path,
                        b"\"",
                    ]),
                );
                return Ok(false);
            }
        }

        if !crate::fsops::move_file(src_c, dst_c) {
            let err_msg = crate::fsops::error_message(crate::fsops::last_errno());
            (self.on_message)(
                Kind::Error,
                &text(&[
                    b"Failed to rename file ",
                    src,
                    b" to ",
                    dst,
                    b": ",
                    &err_msg,
                ]),
            );
            return Ok(false);
        }

        let old_rel = make_relative_name(&self.dest_dir, src);
        let new_rel = make_relative_name(&self.dest_dir, dst);
        (self.on_register_renamed)(&old_rel, &new_rel);
        Ok(true)
    }
    fn ignored_ext(&mut self, name: &[u8]) -> R<bool> {
        let fold = crate::wildmask::Lower::Ascii;
        Ok(crate::util::match_file_ext(
            name,
            &self.ignore_ext,
            b",;",
            &fold,
            &fold,
        ))
    }
}

/// Runs `ParRenamer::Execute` end-to-end on the filesystem using native Rust PAR2 packet
/// parsing, directory traversal, file hashing, and file moving.
#[allow(clippy::too_many_arguments)]
pub fn execute_all(
    info_name: &[u8],
    dest_dir: &[u8],
    ignore_ext: &[u8],
    detect_missing: bool,
    sep: u8,
    is_stopped: &mut dyn FnMut() -> bool,
    on_progress: &mut dyn FnMut(&[u8], i32),
    on_message: &mut dyn FnMut(Kind, &[u8]),
    on_register_parred: &mut dyn FnMut(&[u8]),
    on_register_renamed: &mut dyn FnMut(&[u8], &[u8]),
) -> State {
    let mut env = FsParRenamerEnv {
        dest_dir: dest_dir.to_vec(),
        ignore_ext: ignore_ext.to_vec(),
        dir_list: Vec::new(),
        loaded_entries: Vec::new().into_iter(),
        is_stopped,
        on_progress,
        on_message,
        on_register_parred,
        on_register_renamed,
    };
    let mut state = State {
        detect_missing,
        ..State::default()
    };
    let _ = execute(&mut env, &mut state, info_name, dest_dir, sep);
    state
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn bstring_cut() {
        assert_eq!(join(&[b'a'; 1020], b'/', b"bcdef").len(), 1023);
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
                crate::paths::PATH_SEPARATOR,
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
        buf[64..72].copy_from_slice(MAGIC);
        buf[72..80].copy_from_slice(&0xffff_ffff_ffff_fffcu64.to_le_bytes());
        assert!(parse_par2_packets(&buf).is_none());
    }

    #[test]
    fn fs_par_renamer_rename_does_not_overwrite_destination_on_permission_error() {
        use std::os::unix::fs::PermissionsExt as _;
        if unsafe { libc_geteuid() } == 0 {
            return;
        }
        let dir =
            std::env::temp_dir().join(format!("nzbget_rs_parrenamer_perm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("damaged.par2");
        let dst = dir.join("damaged.par2.bad");
        std::fs::write(&src, b"new-damaged-content").unwrap();
        std::fs::write(&dst, b"original-bad-content").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let mut env = FsParRenamerEnv {
            dest_dir: path_to_bytes(&dir),
            ignore_ext: Vec::new(),
            dir_list: Vec::new(),
            loaded_entries: Vec::new().into_iter(),
            is_stopped: &mut || false,
            on_progress: &mut |_, _| {},
            on_message: &mut |_, _| {},
            on_register_parred: &mut |_| {},
            on_register_renamed: &mut |_, _| {},
        };
        let ok = env
            .rename(&path_to_bytes(&src), &path_to_bytes(&dst))
            .unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!ok);
        assert_eq!(std::fs::read(&dst).unwrap(), b"original-bad-content");
        let _ = std::fs::remove_dir_all(&dir);
    }

    extern "C" {
        #[link_name = "geteuid"]
        fn libc_geteuid() -> u32;
    }
}
