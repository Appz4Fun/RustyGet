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

use std::ffi::{c_char, c_int};

use md5::{Digest, Md5};

extern "C" {
    #[cfg_attr(windows, link_name = "_stricmp")]
    fn strcasecmp(a: *const c_char, b: *const c_char) -> c_int;
    #[cfg_attr(windows, link_name = "_strnicmp")]
    fn strncasecmp(a: *const c_char, b: *const c_char, n: usize) -> c_int;
    fn tolower(c: c_int) -> c_int;
    fn atoi(s: *const c_char) -> c_int;
}

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
    /// ParParser::FilenameFold
    fn fold(&self, byte: u8) -> u8;
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
pub fn execute<E: Env>(env: &mut E, state: &mut State, info_name: &[u8], dest_dir: &[u8], sep: u8) -> R<()> {
    let mut r = ParRenamer { env, state, sep, dest_dir: dest_dir.to_vec(), dir_list: Vec::new(), file_hashes: Vec::new(),
        par_infos: Vec::new(), bad_pars: Vec::new(), loaded_pars: Vec::new() };
    r.run(info_name)
}

impl<E: Env> ParRenamer<'_, E> {
    fn run(&mut self, info_name: &[u8]) -> R<()> {
        self.env.progress(&text(&[b"Checking renamed files for ", info_name]), 0)?;
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
            let env = &*self.env;
            if crate::parparser::parse_par_filename(&full, true, &|b| env.fold(b)).is_some()
                && !self.bad_pars.contains(&full) && !self.loaded_pars.contains(&full) {
                self.load_par_file(&full)?;
            }
        }
        Ok(())
    }

    fn load_par_file(&mut self, path: &[u8]) -> R<()> {
        let Some(count) = self.env.load_par(path)? else {
            self.env.message(Kind::Warning, &text(&[b"Could not load par2-file ", path]))?;
            self.state.has_damaged_par_files = true;
            self.env.state_changed(self.state);
            self.bad_pars.push(path.to_vec());
            return Ok(());
        };

        self.loaded_pars.push(path.to_vec());
        self.env.message(Kind::Info, &text(&[b"Loaded par2-file ", base_name(path), b" for par-rename"]))?;

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
                        self.env.message(Kind::Warning, &text(&[b"Damaged par2-file detected: ", base_name(path)]))?;
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
                        self.file_hashes.push(FileHash { filename: name.to_vec(), hash: hash.to_vec(), exists: false });
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
            self.env.progress(&text(&[b"Checking file ", &name]), stage)?;
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
                self.env.message(Kind::Info, &text(&[b"File ", &name, b" is missing, ignoring"]))?;
            } else {
                self.env.message(Kind::Info, &text(&[b"File ", &name, b" is missing"]))?;
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
            Read::OpenFailed => return self.env.message(Kind::Error, &text(&[b"Could not open file ", path])),
            Read::ReadFailed => return self.env.message(Kind::Error, &text(&[b"Could not read file ", path])),
            Read::Bytes(n) => n.min(buf.len()),
        };
        let hash16k = crate::renameanalyzer::print(&Md5::digest(&buf[..n]));
        let hash16k = &hash16k[..32];

        self.env.debug(&text(&[b"file: ", base_name(path), b"; hash16k: ", hash16k]))?;

        let Some(i) = self.file_hashes.iter().position(|f| f.hash == hash16k) else { return Ok(()) };
        let correct = self.file_hashes[i].filename.clone();
        self.env.debug(&text(&[b"Found correct filename: ", &correct]))?;
        self.file_hashes[i].exists = true;

        let dst = join(dir, self.sep, &correct);
        if !self.env.file_exists(&dst)? && !is_splitted_fragment(path, &correct) {
            self.rename_file(path, &dst)?;
        }
        Ok(())
    }

    fn check_par_file(&mut self, path: &[u8]) -> R<()> {
        self.env.debug(&text(&[b"Checking par2-header for ", path]))?;

        let mut header = [0u8; HEADER];
        match self.env.read(path, &mut header)? {
            Read::OpenFailed => return self.env.message(Kind::Error, &text(&[b"Could not open file ", path])),
            Read::ReadFailed => return self.env.message(Kind::Error, &text(&[b"Could not read file ", path])),
            Read::Bytes(_) => {}
        }

        let length = u64::from_le_bytes(header[8..16].try_into().unwrap());
        if &header[..8] != MAGIC || (HEADER as u64) > length || length & 3 != 0
            || self.env.file_size(path)? < length as u32 as i32 as i64 {
            // not par2-file or damaged header, ignoring the file
            return Ok(());
        }

        let set_id: Vec<u8> = crate::renameanalyzer::print(&header[32..48])[..32].iter()
            .map(|&b| unsafe { tolower(b as c_int) } as u8).collect();

        self.env.debug(&text(&[b"Storing: ", base_name(path), b"; setid: ", &set_id]))?;
        self.par_infos.push(ParInfo { filename: path.to_vec(), set_id });
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
        crate::parparser::parse_par_filename(name, true, &|b| self.env.fold(b)).map_or(0, |(len, _)| len)
    }

    fn need_rename_par_files(&self) -> bool {
        for (i, par1) in self.par_infos.iter().enumerate() {
            let base1 = c(base_name(&par1.filename));
            let Some(dot) = base1[..base1.len() - 1].iter().rposition(|&b| b == b'.') else { return true };
            if unsafe { strcasecmp(base1[dot..].as_ptr().cast(), c".par2".as_ptr()) } != 0 {
                // file doesn't have "par2" extension
                return true;
            }
            let len1 = self.base_len(&base1[..base1.len() - 1]);
            for par2 in &self.par_infos[i + 1..] {
                if par1.set_id == par2.set_id {
                    let base2 = c(base_name(&par2.filename));
                    let len2 = self.base_len(&base2[..base2.len() - 1]);
                    if len1 != len2 || unsafe { strncasecmp(base1.as_ptr().cast(), base2.as_ptr().cast(), len1) } != 0 {
                        // same setid but different base file names
                        return true;
                    }
                }
            }
        }
        false
    }

    fn rename_par_file(&mut self, dir: &[u8], filename: &[u8], set_id: &[u8]) -> R<()> {
        self.env.debug(&text(&[b"Renaming: ", base_name(filename), b"; setid: ", set_id]))?;
        // the first name, then the next one while the last one exists
        let mut num: i32 = 1;
        loop {
            let dest = bstring([dir, &[self.sep], set_id, format!(".vol{num:03}+01.PAR2").as_bytes()].concat());
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
/// with NNN (all digits) at most 1 (.000 or .001; atoi's value).
pub fn is_splitted_fragment(filename: &[u8], correct_name: &[u8]) -> bool {
    let base = c(base_name(filename));
    let correct = c(correct_name);
    let base_len = correct.len() - 1;
    let has_extension = base[..base.len() - 1].contains(&b'.');
    if !has_extension || unsafe { strncasecmp(base.as_ptr().cast(), correct.as_ptr().cast(), base_len) } != 0 {
        return false;
    }
    // strncasecmp matched base_len bytes, none of them the terminator
    if base.get(base_len) != Some(&b'.') {
        return false;
    }
    let digits = &base[base_len + 1..base.len() - 1];
    digits.iter().all(u8::is_ascii_digit) && unsafe { atoi(base[base_len + 1..].as_ptr().cast()) } <= 1
}

/// Returns true when `ParRenamer::RenameFile` needs to create target directories (`old_path != new_path`).
pub fn par_renamer_needs_create_target_dir(old_path: &[u8], new_path: &[u8]) -> bool {
    old_path != new_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitted_fragments() {
        assert!(is_splitted_fragment(b"/d/movie.mkv.001", b"movie.mkv"));
        assert!(is_splitted_fragment(b"/d/MOVIE.MKV.000", b"movie.mkv"));
        assert!(is_splitted_fragment(b"/d/movie.mkv.", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/movie.mkv.002", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/movie.mkv.001x", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/movie.mkv", b"movie.mkv"));
        assert!(!is_splitted_fragment(b"/d/other.001", b"movie.mkv"));
        let overflow = c"99999999999999999999999";
        assert_eq!(is_splitted_fragment(b"/d/a.99999999999999999999999", b"a"),
            unsafe { atoi(overflow.as_ptr()) } <= 1);
    }

    #[test]
    fn bstring_cut() {
        assert_eq!(join(&[b'a'; 1020], b'/', b"bcdef").len(), 1023);
    }

    fn latin1_to_utf8(latin1: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(latin1.len() * 2);
        for &ch in latin1 {
            if ch < 128 {
                out.push(ch);
            } else {
                out.push(0xc2 + u8::from(ch > 0xbf));
                out.push((ch & 0x3f) + 0x80);
            }
        }
        out
    }

    fn url_encode_char(ch: u8) -> [u8; 3] {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        [b'%', HEX[(ch >> 4) as usize], HEX[(ch & 0x0f) as usize]]
    }

    fn translate_filename_from_par2_to_local(par2_name: &[u8]) -> Vec<u8> {
        let mut local = Vec::with_capacity(par2_name.len());
        for &raw in par2_name {
            let mut ch = raw;
            let ok = ch >= 32;
            if ch == b'\\' {
                ch = b'/';
            }
            if ok {
                local.push(ch);
            } else {
                local.extend_from_slice(&url_encode_char(ch));
            }
        }
        if local.first() == Some(&b'/') {
            let enc = url_encode_char(b'/');
            local.splice(0..1, enc);
        }
        while let Some(idx) = local.windows(3).position(|w| w == b"../") {
            let dot = url_encode_char(b'.');
            let mut rep = Vec::with_capacity(6);
            rep.extend_from_slice(&dot);
            rep.extend_from_slice(&dot);
            local.splice(idx..idx + 2, rep);
        }
        local
    }

    fn parse_par2_packets(bytes: &[u8]) -> Option<Vec<ParEntry>> {
        if bytes.is_empty() {
            return None;
        }
        const FILE_DESC_TYPE: &[u8; 16] = b"PAR 2.0\0FileDesc";
        const IFSC_TYPE: &[u8; 16] = b"PAR 2.0\0IFSC\0\0\0\0";
        const MAIN_TYPE: &[u8; 16] = b"PAR 2.0\0Main\0\0\0\0";
        const CREATOR_TYPE: &[u8; 16] = b"PAR 2.0\0Creator\0";
        const RECV_TYPE: &[u8; 16] = b"PAR 2.0\0RecvSlic";

        // Key is reversed fileid so BTreeMap orders identically to Par2::MD5Hash::operator<
        let mut source_map: std::collections::BTreeMap<[u8; 16], Option<(Vec<u8>, Vec<u8>)>> =
            std::collections::BTreeMap::new();
        let mut set_id: Option<[u8; 16]> = None;
        let mut packets = 0usize;
        let mut offset = 0usize;

        while offset + HEADER <= bytes.len() {
            if &bytes[offset..offset + 8] != MAGIC {
                offset += 1;
                if let Some(pos) = bytes[offset..].windows(8).position(|w| w == MAGIC) {
                    offset += pos;
                } else {
                    break;
                }
                if offset + HEADER > bytes.len() {
                    break;
                }
            }
            let length = u64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().unwrap()) as usize;
            if length < HEADER || (length & 3) != 0 || offset + length > bytes.len() {
                offset += 1;
                continue;
            }
            let digest = Md5::digest(&bytes[offset + 32..offset + length]);
            if digest.as_slice() != &bytes[offset + 16..offset + 32] {
                offset += 1;
                continue;
            }
            let pkt_set_id: [u8; 16] = bytes[offset + 32..offset + 48].try_into().unwrap();
            if set_id.is_none() {
                set_id = Some(pkt_set_id);
            }
            if set_id == Some(pkt_set_id) {
                let pkt_type = &bytes[offset + 48..offset + 64];
                if pkt_type == FILE_DESC_TYPE {
                    if length > 120 && length - 120 <= 100_000 {
                        let mut key: [u8; 16] = bytes[offset + 64..offset + 80].try_into().unwrap();
                        key.reverse();
                        let hashfull = &bytes[offset + 80..offset + 96];
                        let hash16k = &bytes[offset + 96..offset + 112];
                        let file_len = u64::from_le_bytes(bytes[offset + 112..offset + 120].try_into().unwrap());
                        if !(file_len <= 16384 && hash16k != hashfull) {
                            let raw_name = &bytes[offset + 120..offset + length];
                            let nul = raw_name.iter().position(|&b| b == 0).unwrap_or(raw_name.len());
                            let utf8_name = latin1_to_utf8(&raw_name[..nul]);
                            let local_name = translate_filename_from_par2_to_local(&utf8_name);
                            let printed_hash = crate::renameanalyzer::print(hash16k)[..32].to_vec();
                            let entry = source_map.entry(key).or_insert(None);
                            if entry.is_none() {
                                *entry = Some((local_name, printed_hash));
                                packets += 1;
                            }
                        }
                    }
                } else if pkt_type == IFSC_TYPE {
                    if length >= 80 && (length - 80) % 20 == 0 {
                        let mut key: [u8; 16] = bytes[offset + 64..offset + 80].try_into().unwrap();
                        key.reverse();
                        source_map.entry(key).or_insert(None);
                        packets += 1;
                    }
                } else if pkt_type == MAIN_TYPE || pkt_type == CREATOR_TYPE || pkt_type == RECV_TYPE {
                    packets += 1;
                }
            }
            offset += length;
        }
        let _ = packets;
        Some(
            source_map
                .into_values()
                .map(|opt| match opt {
                    Some((name, hash)) => ParEntry::File { name, hash },
                    None => ParEntry::Damaged,
                })
                .collect(),
        )
    }

    struct FsParRenamerEnv {
        dest_dir: Vec<u8>,
        dir_list: Vec<Vec<u8>>,
        loaded_entries: std::vec::IntoIter<ParEntry>,
    }

    impl FsParRenamerEnv {
        fn new(dest_dir: &std::path::Path) -> Self {
            Self {
                dest_dir: dest_dir.to_string_lossy().as_bytes().to_vec(),
                dir_list: Vec::new(),
                loaded_entries: Vec::new().into_iter(),
            }
        }
    }

    impl Env for FsParRenamerEnv {
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
            Ok(false)
        }
        fn progress(&mut self, _label: &[u8], _stage: i32) -> R<()> {
            Ok(())
        }
        fn message(&mut self, _kind: Kind, _text: &[u8]) -> R<()> {
            Ok(())
        }
        fn debug(&mut self, _text: &[u8]) -> R<()> {
            Ok(())
        }
        fn list_dir(&mut self, dir: &[u8]) -> R<Vec<Vec<u8>>> {
            let path = std::path::Path::new(std::str::from_utf8(dir).unwrap());
            let mut out = Vec::new();
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::ffi::OsStrExt;
                        out.push(entry.file_name().as_bytes().to_vec());
                    }
                    #[cfg(not(unix))]
                    {
                        out.push(entry.file_name().to_string_lossy().as_bytes().to_vec());
                    }
                }
            }
            Ok(out)
        }
        fn dir_exists(&mut self, path: &[u8]) -> R<bool> {
            let p = std::path::Path::new(std::str::from_utf8(path).unwrap());
            Ok(p.is_dir())
        }
        fn file_exists(&mut self, path: &[u8]) -> R<bool> {
            let p = std::path::Path::new(std::str::from_utf8(path).unwrap());
            Ok(p.is_file())
        }
        fn file_size(&mut self, path: &[u8]) -> R<i64> {
            let p = std::path::Path::new(std::str::from_utf8(path).unwrap());
            Ok(std::fs::metadata(p).map(|m| m.len() as i64).unwrap_or(-1))
        }
        fn read(&mut self, path: &[u8], buf: &mut [u8]) -> R<Read> {
            use std::io::Read as _;
            let p = std::path::Path::new(std::str::from_utf8(path).unwrap());
            let Ok(mut f) = std::fs::File::open(p) else {
                return Ok(Read::OpenFailed);
            };
            match f.read(buf) {
                Ok(n) => Ok(Read::Bytes(n)),
                Err(_) => Ok(Read::ReadFailed),
            }
        }
        fn load_par(&mut self, path: &[u8]) -> R<Option<usize>> {
            let p = std::path::Path::new(std::str::from_utf8(path).unwrap());
            let Ok(bytes) = std::fs::read(p) else {
                return Ok(None);
            };
            let Some(entries) = parse_par2_packets(&bytes) else {
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
            let p = std::path::Path::new(std::str::from_utf8(dir).unwrap());
            let mut list = Vec::new();
            let fold = |b: u8| b.to_ascii_lowercase();
            crate::parparser::find_main_pars(p, true, &fold, |name| {
                list.push(name.to_vec());
            });
            Ok(list)
        }
        fn register_parred(&mut self, _name: &[u8]) -> R<()> {
            Ok(())
        }
        fn rename(&mut self, src: &[u8], dst: &[u8]) -> R<bool> {
            let s = std::path::Path::new(std::str::from_utf8(src).unwrap());
            let d = std::path::Path::new(std::str::from_utf8(dst).unwrap());
            if let (Some(sp), Some(dp)) = (s.parent(), d.parent()) {
                if sp != dp {
                    let _ = std::fs::create_dir_all(dp);
                }
            }
            Ok(std::fs::rename(s, d).is_ok())
        }
        fn ignored_ext(&mut self, _name: &[u8]) -> R<bool> {
            Ok(false)
        }
        fn fold(&self, byte: u8) -> u8 {
            byte.to_ascii_lowercase()
        }
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

        let run_case = |name: &str, src_dir: &std::path::Path, detect_missing: bool, setup: &dyn Fn(&std::path::Path)| -> State {
            let work_dir = std::env::temp_dir().join(format!("nzbget_rs_parrenamer_{}_{}", std::process::id(), name));
            copy_dir(src_dir, &work_dir);
            setup(&work_dir);
            let mut env = FsParRenamerEnv::new(&work_dir);
            let mut state = State {
                detect_missing,
                ..State::default()
            };
            let dest_bytes = work_dir.to_string_lossy().as_bytes().to_vec();
            execute(&mut env, &mut state, b"test", &dest_bytes, crate::paths::PATH_SEPARATOR).unwrap();
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
    }
}
