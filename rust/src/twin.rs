//! Twins and alts: duplicates of a download told apart by the files their par2
//! sets describe. A twin holds files byte-identical to the primary's (its
//! articles, par2-files and bytes can stand in for the primary's); a near-twin
//! shares most of them (a re-packed archive header); an alt is another encode.
//!
//! The C++ side (TwinCheck) fetches articles and walks the queue; everything
//! else lives here: reading nzb-files, parsing par2 FileDesc packets,
//! fingerprints, the per-posting file lists kept in the queue
//! directory, labels, and picking a duplicate's file by content.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::sync::Mutex;

/// One file of a par2 set: what identifies its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSig {
    pub name: String,
    pub length: u64,
    /// MD5 of the whole file, lower-case hex
    pub md5: String,
}

/// PAR2 packet types the walk reads (the 16 bytes after the set id).
const TYPE_MAIN: &[u8; 16] = b"PAR 2.0\0Main\0\0\0\0";
const TYPE_FILE_DESC: &[u8; 16] = b"PAR 2.0\0FileDesc";
const PACKET_MAGIC: &[u8; 8] = b"PAR2\0PKT";

// File IDs come from untrusted PAR2 packets: use the standard keyed,
// randomized hasher and include every byte of each ID.
type IdMap<V> = HashMap<[u8; 16], V>;

/// The first packet magic in `hay`: memchr for its first byte (SIMD, and
/// rare in par2 data), then the other 7 compared.
fn find_magic(hay: &[u8]) -> Option<usize> {
    let mut at = 0;
    while let Some(p) = crate::bytes::find_byte(&hay[at..], PACKET_MAGIC[0]) {
        let start = at + p;
        match hay.get(start..start + 8) {
            Some(window) if window == PACKET_MAGIC => return Some(start),
            Some(_) => at = start + 1,
            None => return None,
        }
    }
    None
}

/// What the walk of a par2-file's packets found: the Main packet's block
/// size and file ids, and the FileDesc packets' files by file id.
struct Walked {
    slice_size: u64,
    main_valid: bool,
    file_order: Vec<[u8; 16]>,
    files: IdMap<FileSig>,
}

/// The packets of a par2-file, framed and read as rust-par2's parser
/// (parse_par2_reader) reads them: the packet magic found wherever it is,
/// lengths a multiple of 4 and at least 64, a damaged packet (its MD5 not
/// the one in its header) skipped by its length, a Main packet's block size
/// and all file ids taken from the last intact one (including non-recoverable
/// ids after the recoverable count), a FileDesc packet from the
/// first intact one of its file id. Only the packets that make the result
/// get their MD5 checked: the parser also read and hashed every IFSC,
/// recovery and creator packet, and megabytes of recovery blocks, for
/// nothing in the result. `files` is false when only the block size is
/// wanted. None where the parser had an error: no intact Main packet, or a
/// packet cut short inside its header. A packet whose length runs past the
/// data ends the walk. The flag is whether a packet magic was seen at all.
fn walk(data: &[u8], files: bool) -> (Option<Walked>, bool) {
    use md5::{Digest, Md5};
    let mut walked = Walked {
        slice_size: 0,
        main_valid: false,
        file_order: Vec::new(),
        files: IdMap::default(),
    };
    let mut have_main = false;
    let mut seen = false;
    let mut pos = 0usize;
    while pos < data.len() {
        // the magic; elsewhere, the next one
        let Some(magic) = data.get(pos..pos + 8) else {
            break;
        };
        if magic != PACKET_MAGIC {
            match find_magic(&data[pos + 8..]) {
                Some(at) => {
                    pos += 8 + at;
                    continue;
                }
                None => break,
            }
        }
        seen = true;
        let Some(length) = data.get(pos + 8..pos + 16) else {
            break;
        };
        let length = u64::from_le_bytes(length.try_into().unwrap());
        pos += 16;
        if length < 64 || length % 4 != 0 {
            continue;
        }
        let rest = (data.len() - pos) as u64;
        if length - 16 > 10 * 1024 * 1024 {
            // a recovery slice's size: only its header is read, then it is skipped
            if rest < 48 {
                return (None, true);
            }
            if length - 16 > rest {
                break;
            }
            pos += (length - 16) as usize;
            continue;
        }
        // the header's MD5, and the rest of the packet
        if rest < 16 {
            return (None, true);
        }
        if length - 16 > rest {
            break;
        }
        let stored = &data[pos..pos + 16];
        let packet = &data[pos + 16..pos + (length - 16) as usize];
        pos += (length - 16) as usize;
        let kind = &packet[16..32];
        let main = kind == TYPE_MAIN;
        if !main && !(files && kind == TYPE_FILE_DESC) {
            continue;
        }
        if Md5::digest(packet).as_slice() != stored {
            continue;
        }
        if main {
            if packet.len() < 44 {
                continue;
            }
            // The count field covers recoverable files only. Non-recoverable
            // IDs follow them and also require FileDesc packets for completeness.
            let count = (packet.len() - 44) / 16;
            let recoverable = u32::from_le_bytes(packet[40..44].try_into().unwrap()) as usize;
            // A packet checksum does not validate its file count or ID array.
            // Keep any usable descriptions, but this Main cannot prove completeness.
            walked.main_valid = (packet.len() - 44).is_multiple_of(16) && recoverable <= count;
            walked.slice_size = u64::from_le_bytes(packet[32..40].try_into().unwrap());
            walked.file_order = (0..count)
                .map(|i| packet[44 + 16 * i..60 + 16 * i].try_into().unwrap())
                .collect();
            have_main = true;
        } else if packet.len() >= 88 {
            let id: [u8; 16] = packet[32..48].try_into().unwrap();
            walked.files.entry(id).or_insert_with(|| {
                let name = &packet[88..];
                let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
                FileSig {
                    name: String::from_utf8_lossy(name).into_owned(),
                    length: u64::from_le_bytes(packet[80..88].try_into().unwrap()),
                    md5: hex(&packet[48..64]),
                }
            });
        }
    }
    (have_main.then_some(walked), seen)
}

/// The files the FileDesc packets in `data` describe (a par2-file, or the part
/// of one that arrived: lost articles leave gaps, the parser skips to the next
/// packet). Empty when there's no usable set. Names are the PAR2's own (twin
/// par-repair compares them); matching makes them valid file names first.
pub fn par2_sigs(data: &[u8]) -> Vec<FileSig> {
    par2_set(data).0
}

/// The files of the par2 set in `data`, and whether the list is complete: the
/// Main packet has whole file IDs and a consistent recoverable count, and every
/// file it names has its FileDesc packet. A
/// partial list proves nothing about a file it doesn't name.
pub fn par2_set(data: &[u8]) -> (Vec<FileSig>, bool) {
    let (mut sigs, complete) = match walk(data, true).0 {
        Some(set) if !set.files.is_empty() => {
            let complete = set.main_valid
                && !set.file_order.is_empty()
                && set.file_order.iter().all(|id| set.files.contains_key(id));
            (set.files.into_values().collect(), complete)
        }
        // its Main packet lost (the first article of the par2-file missing): the
        // FileDesc packets that arrived still tell the files, but maybe not all
        _ => (scan_file_descs(data), false),
    };
    sigs.sort_by(|a, b| a.name.cmp(&b.name));
    (sigs, complete)
}

fn valid_name(name: &str) -> String {
    String::from_utf8_lossy(&crate::paths::make_valid_filename(name.as_bytes(), false)).into_owned()
}

/// Whether a packet's bytes are its own: the MD5 in its header (bytes 16-31)
/// covers the rest from the set id on. A lost article leaves zeros that pass
/// the magic and length checks, not this one.
fn packet_intact(packet: &[u8]) -> bool {
    use md5::{Digest, Md5};
    let digest = Md5::digest(&packet[32..]);
    digest.as_slice() == &packet[16..32]
}

/// FileDesc packets read wherever they are in `data` (no Main packet needed),
/// each checked against its MD5.
fn scan_file_descs(data: &[u8]) -> Vec<FileSig> {
    let mut files: HashMap<[u8; 16], FileSig> = HashMap::new();
    let mut pos = 0usize;
    while pos + 64 <= data.len() {
        if &data[pos..pos + 8] != b"PAR2\0PKT" {
            pos += 4;
            continue;
        }
        let length = u64::from_le_bytes(data[pos + 8..pos + 16].try_into().unwrap()) as usize;
        if length < 64
            || !length.is_multiple_of(4)
            || length > data.len() - pos
            || !packet_intact(&data[pos..pos + length])
        {
            pos += 4;
            continue;
        }
        if &data[pos + 48..pos + 64] == b"PAR 2.0\0FileDesc" && length >= 64 + 56 {
            // file id, MD5 of the file, MD5 of its first 16 KB, length, name
            let body = &data[pos + 64..pos + length];
            let id: [u8; 16] = body[0..16].try_into().unwrap();
            let name = &body[56..];
            let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
            files.insert(id, FileSig {
                name: String::from_utf8_lossy(name).into_owned(),
                length: u64::from_le_bytes(body[48..56].try_into().unwrap()),
                md5: hex(&body[16..32]),
            });
        }
        pos += length;
    }
    let mut sigs: Vec<FileSig> = files.into_values().collect();
    sigs.sort_by(|a, b| a.name.cmp(&b.name));
    sigs
}

/// The block size of the par2 set in `data` (its Main packet), 0 if none.
pub fn block_size(data: &[u8]) -> u64 {
    let (set, seen) = walk(data, false);
    if let Some(set) = set {
        if set.slice_size > 0 {
            return set.slice_size;
        }
    }
    // the walk saw no packet magic: it looks from the 9th byte on, so only a
    // magic in the first 8 bytes' shadow (at 4) is left for the scan below
    let data = if seen {
        data
    } else {
        &data[..data.len().min(80)]
    };
    // the set didn't parse: the Main packet alone may still be there
    let mut pos = 0usize;
    while pos + 72 <= data.len() {
        if &data[pos..pos + 8] == b"PAR2\0PKT"
            && &data[pos + 48..pos + 64] == b"PAR 2.0\0Main\0\0\0\0"
        {
            return u64::from_le_bytes(data[pos + 64..pos + 72].try_into().unwrap());
        }
        pos += 4;
    }
    0
}

/// The recovery blocks a par2-volume's name gives ("x.vol03+04.par2": 4), None if none.
pub fn volume_blocks(filename: &str) -> Option<u32> {
    volume_blocks_bytes(filename.as_bytes())
}

/// `volume_blocks` over the name's bytes (no copy: the last ".vol", in any
/// case, then the first '+' after it, then the digits that follow).
pub fn volume_blocks_bytes(name: &[u8]) -> Option<u32> {
    let at = (0..name.len().checked_sub(3)?)
        .rev()
        .find(|&i| name[i] == b'.' && name[i + 1..i + 4].eq_ignore_ascii_case(b"vol"))?;
    let plus = at + crate::bytes::find_byte(&name[at..], b'+')?;
    let mut blocks: u32 = 0;
    let mut any = false;
    for &b in &name[plus + 1..] {
        if !b.is_ascii_digit() {
            break;
        }
        blocks = blocks.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
        any = true;
    }
    any.then_some(blocks)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

/// The posting's fingerprint: equal for postings of byte-identical files,
/// whatever their names. FNV-1a over the sorted "md5:length" lines, so the
/// values TwinCheck stored before (in C++) stay valid. "" for no files.
pub fn fingerprint(sigs: &[FileSig]) -> String {
    fingerprint_of(sigs.iter().map(|s| (s.md5.as_str(), s.length)))
}

/// Compares a candidate twin's PAR2 bytes (`twin_data`) against our PAR2 set's
/// `own_names`, `own_md5s`, and `own_print` for `RepairController::PostParChecker::RequestTwinIndex`:
/// - `0`: exact twin PAR2 set match (same fingerprint and identical `name/md5` set)
/// - `1`: `"describes other files"` (`fingerprint(sigs) != own_print`)
/// - `2`: `"names the files otherwise"` (`fingerprint(sigs) == own_print`, but `name/md5` set differs)
pub fn compare_twin_par2_data(
    own_names: &[&[u8]],
    own_md5s: &[&[u8]],
    own_print: &[u8],
    twin_data: &[u8],
) -> i32 {
    let sigs = par2_sigs(twin_data);
    let twin_print = fingerprint(&sigs);
    if twin_print.as_bytes() != own_print {
        return 1;
    }
    let count = own_names.len().min(own_md5s.len());
    let mut own_set: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
    for i in 0..count {
        let mut entry = Vec::with_capacity(own_names[i].len() + 1 + own_md5s[i].len());
        entry.extend_from_slice(own_names[i]);
        entry.push(b'/');
        entry.extend_from_slice(own_md5s[i]);
        own_set.insert(entry);
    }
    let mut twin_set: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
    for sig in &sigs {
        let mut entry = Vec::with_capacity(sig.name.len() + 1 + sig.md5.len());
        entry.extend(sig.name.bytes().map(|b| {
            if matches!(b, b'\n' | b'\t' | b'\r') {
                b'_'
            } else {
                b
            }
        }));
        entry.push(b'/');
        entry.extend_from_slice(sig.md5.as_bytes());
        twin_set.insert(entry);
    }
    if twin_set != own_set {
        2
    } else {
        0
    }
}

/// `fingerprint` over (md5, length) pairs: the sorted, distinct "md5:length"
/// lines hashed, without a string for each.
pub fn fingerprint_of<'a>(files: impl Iterator<Item = (&'a str, u64)>) -> String {
    fingerprint_cow(files.map(|(md5, length)| (std::borrow::Cow::Borrowed(md5), length)))
}

pub fn fingerprint_cow<'a>(
    files: impl Iterator<Item = (std::borrow::Cow<'a, str>, u64)>,
) -> String {
    let mut arena: Vec<u8> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for (md5, length) in files {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        let mut rest = length;
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        let start = arena.len();
        arena.extend_from_slice(md5.as_bytes());
        arena.push(b':');
        arena.extend_from_slice(&digits[at..]);
        spans.push((start, arena.len()));
    }
    // the sorted, distinct "md5:length" lines, as a BTreeSet<String> held them
    spans.sort_unstable_by(|a, b| arena[a.0..a.1].cmp(&arena[b.0..b.1]));
    spans.dedup_by(|a, b| arena[a.0..a.1] == arena[b.0..b.1]);
    if spans.is_empty() {
        return String::new();
    }
    let mut hash: u64 = 14695981039346656037;
    for &(start, end) in &spans {
        for &byte in arena[start..end].iter().chain(std::iter::once(&b'\n')) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(1099511628211);
        }
    }
    format!("{hash:016x}-{}", spans.len())
}

/// How many of `primary`'s files `dupe` holds (by MD5 and length), each file
/// of `dupe` standing in for one of `primary`'s at most.
pub fn shared_files(primary: &[FileSig], dupe: &[FileSig]) -> usize {
    let mut held: HashMap<(&str, u64), usize> = HashMap::new();
    for s in dupe {
        *held.entry((s.md5.as_str(), s.length)).or_default() += 1;
    }
    primary
        .iter()
        .filter(|s| match held.get_mut(&(s.md5.as_str(), s.length)) {
            Some(count) if *count > 0 => {
                *count -= 1;
                true
            }
            _ => false,
        })
        .count()
}

/// The label of `dupe` against `primary`: "twin" (every file, and no other),
/// "twin:N/M" (N of the primary's M files, more than half: a near-twin, a
/// re-packed archive header), "alt" (fewer: a shared NFO or sample doesn't make
/// a twin), "" when a list is empty, or when an incomplete list would make an alt.
pub fn kind(primary: &Listed, dupe: &Listed) -> String {
    if primary.sigs.is_empty() || dupe.sigs.is_empty() {
        return String::new();
    }
    let shared = shared_files(&primary.sigs, &dupe.sigs);
    let total = primary.sigs.len();
    if primary.complete && dupe.complete && shared == total && dupe.sigs.len() == total {
        "twin".into()
    } else if primary.complete && shared * 2 > total {
        // the primary's complete list gives the denominator; the donor's may lack
        // files it has (more of them only makes it more of a twin)
        format!("twin:{shared}/{total}")
    } else if primary.complete && dupe.complete {
        "alt".into()
    } else {
        String::new()
    }
}

/// The files of two partial lists of one posting together (each file once):
/// retries that saw other articles of the par2-file add what they found.
pub fn merge(a: &[FileSig], b: &[FileSig]) -> Vec<FileSig> {
    let mut seen: std::collections::HashSet<(&str, &str, u64)> = std::collections::HashSet::new();
    let mut merged: Vec<FileSig> = Vec::with_capacity(a.len() + b.len());
    for sig in a.iter().chain(b) {
        if seen.insert((sig.name.as_str(), sig.md5.as_str(), sig.length)) {
            merged.push(sig.clone());
        }
    }
    merged.sort_by(|x, y| x.name.cmp(&y.name));
    merged
}

/// The name of the duplicate's file holding the same bytes as our file named
/// `target` (or `target_alt`, the name our nzb gives it). `None`: not known (our
/// list is partial or doesn't have the file, the name fits several, several donor files hold the
/// bytes, or the donor's list is incomplete and doesn't have them). `Some("")`:
/// the donor's complete list has no file with these bytes.
pub fn match_by_content(
    own: &Listed,
    target: &str,
    target_alt: &str,
    donor: &Listed,
) -> Option<String> {
    // our name is known to be unique only in our complete list: a FileDesc the
    // partial one lacks may make the same valid name
    if !own.complete {
        return None;
    }
    let ours = find_sig(&own.sigs, target).or_else(|| find_sig(&own.sigs, target_alt))?;
    let mut found = donor
        .sigs
        .iter()
        .filter(|s| s.md5 == ours.md5 && s.length == ours.length);
    match (found.next(), found.next()) {
        (Some(one), None) => Some(valid_name(&one.name)),
        (Some(_), Some(_)) => None,
        (None, _) if donor.complete => Some(String::new()),
        (None, _) => None,
    }
}

/// The file named `name`: the one with this exact name, else the only one that
/// differs in case; `None` when there's none or the case leaves it open.
fn find_sig<'a>(sigs: &'a [FileSig], name: &str) -> Option<&'a FileSig> {
    if name.is_empty() {
        return None;
    }
    // the names the queue gives files: PAR2 names made valid file names. Two
    // PAR2 names can make one valid name ("a:b.rar", "a?b.rar"): then it's open
    let mut exact = sigs.iter().filter(|s| valid_name(&s.name) == name);
    match (exact.next(), exact.next()) {
        (Some(one), None) => return Some(one),
        (Some(_), Some(_)) => return None,
        _ => {}
    }
    let mut folded = sigs
        .iter()
        .filter(|s| valid_name(&s.name).eq_ignore_ascii_case(name));
    match (folded.next(), folded.next()) {
        (Some(one), None) => Some(one),
        _ => None,
    }
}

/// A posting's file list as stored: bound to the fingerprint it was made for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listed {
    pub sigs: Vec<FileSig>,
    pub complete: bool,
    pub print: String,
}

/// The par2 file lists of the postings fingerprinted so far, by download id,
/// appended to a file in the queue directory. A record is a line
/// "id\tnew\tfingerprint\tcomplete", its "id\tlength\tmd5\tname" lines and a
/// line "id\tend\tcount": a record cut short (a full disk) has no end line with
/// its count and is ignored. A later record of an id replaces the earlier one.
/// A list is only used for the posting it was made for, given by its identity
/// (the C++ side passes the fingerprint and the posting's message-id hash): a
/// fingerprint alone is shared by differently named twins, and download ids
/// start over when the queue and history are emptied while the file stays
/// (`prune` keeps it to the postings still there).
#[derive(Default)]
pub struct Store {
    path: std::path::PathBuf,
    lists: HashMap<i32, Listed>,
}

static EMPTY: Listed = Listed {
    sigs: Vec::new(),
    complete: false,
    print: String::new(),
};

fn record(id: i32, listed: &Listed) -> String {
    let mut text = format!(
        "{id}\tnew\t{}\t{}\n",
        listed.print,
        u8::from(listed.complete)
    );
    for s in &listed.sigs {
        let name = s.name.replace(['\n', '\t', '\r'], "_");
        text.push_str(&format!("{id}\t{}\t{}\t{name}\n", s.length, s.md5));
    }
    text.push_str(&format!("{id}\tend\t{}\n", listed.sigs.len()));
    text
}

impl Store {
    pub fn open(path: std::path::PathBuf) -> Store {
        let mut store = Store {
            path,
            lists: HashMap::new(),
        };
        let mut pending: HashMap<i32, Listed> = HashMap::new();
        if let Ok(file) = std::fs::File::open(&store.path) {
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                let parts: Vec<&str> = line.splitn(4, '\t').collect();
                let Some(Ok(id)) = parts.first().map(|id| id.parse::<i32>()) else {
                    continue;
                };
                match parts.get(1) {
                    Some(&"new") => {
                        pending.insert(
                            id,
                            Listed {
                                sigs: Vec::new(),
                                print: parts.get(2).unwrap_or(&"").to_string(),
                                complete: parts.get(3) == Some(&"1"),
                            },
                        );
                    }
                    Some(&"end") => {
                        let count = parts.get(2).and_then(|c| c.parse::<usize>().ok());
                        if let Some(listed) = pending.remove(&id) {
                            if count == Some(listed.sigs.len()) {
                                store.lists.insert(id, listed);
                            }
                        }
                    }
                    _ => {
                        let (Some(length), Some(md5), Some(name)) =
                            (parts.get(1), parts.get(2), parts.get(3))
                        else {
                            continue;
                        };
                        let Ok(length) = length.parse::<u64>() else {
                            continue;
                        };
                        if let Some(listed) = pending.get_mut(&id) {
                            listed.sigs.push(FileSig {
                                name: name.to_string(),
                                length,
                                md5: md5.to_string(),
                            });
                        }
                    }
                }
            }
        }
        store
    }

    pub fn put(&mut self, id: i32, listed: Listed) {
        if let Ok(mut out) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = out.write_all(record(id, &listed).as_bytes());
        }
        self.lists.insert(id, listed);
    }

    /// A partial list of download `id` stored for the posting `posting` (the part
    /// of its identity after '#'): retries add to it until a complete list comes.
    pub fn partial_of(&self, id: i32, posting: &str) -> Option<&Listed> {
        self.lists.get(&id).filter(|listed| {
            !listed.complete && listed.print.rsplit_once('#').map(|(_, p)| p) == Some(posting)
        })
    }

    /// The list of download `id` if it was made for posting identity `print`, else an empty one.
    pub fn get(&self, id: i32, print: &str) -> &Listed {
        match self.lists.get(&id) {
            Some(listed) if !print.is_empty() && listed.print == print => listed,
            _ => &EMPTY,
        }
    }

    /// Keeps the lists of the postings still in queue or history (`live`: id and
    /// fingerprint), the rest dropped, and rewrites the file with them (to a
    /// temporary file renamed over it: never half written).
    pub fn prune(&mut self, live: &[(i32, String)]) {
        let keep: HashMap<i32, &str> = live
            .iter()
            .map(|(id, print)| (*id, print.as_str()))
            .collect();
        self.lists
            .retain(|id, listed| keep.get(id) == Some(&listed.print.as_str()));
        let mut ids: Vec<&i32> = self.lists.keys().collect();
        ids.sort();
        let text: String = ids.iter().map(|id| record(**id, &self.lists[id])).collect();
        let mut temp = self.path.clone().into_os_string();
        temp.push(".new");
        let temp = std::path::PathBuf::from(temp);
        // rename replaces the file in one step, also on Windows (MoveFileExW with
        // MOVEFILE_REPLACE_EXISTING), so a reader sees the old or the new store
        if std::fs::write(&temp, text).is_ok() {
            let _ = std::fs::rename(&temp, &self.path);
        }
    }
}

/// The one store of the process (the queue directory's); `None` until opened.
pub static STORE: Mutex<Option<Store>> = Mutex::new(None);

/// One `<file>` of an nzb-file: what fetching it needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NzbEntry {
    pub subject: String,
    /// the quoted name in the subject, path separators replaced; "" if none
    pub filename: String,
    pub groups: Vec<String>,
    /// bytes and message-id (with angle brackets, sanitized for NNTP commands)
    pub segments: Vec<(u64, String)>,
}

impl NzbEntry {
    pub fn size(&self) -> u64 {
        self.segments.iter().map(|s| s.0).sum()
    }
    pub fn is_par2(&self) -> bool {
        let name = if self.filename.is_empty() {
            &self.subject
        } else {
            &self.filename
        };
        name.to_ascii_lowercase().contains(".par2")
    }
}

/// The `<file>` entries of an nzb-file, read without the queue's parser (that
/// one writes the article lists to the queue directory in server mode).
// the replacement needs the XML version, which nzb-files don't vary
#[allow(deprecated)]
pub fn read_nzb(xml: &[u8]) -> Vec<NzbEntry> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut entries = Vec::new();
    let mut entry: Option<NzbEntry> = None;
    let mut text = String::new();
    let mut bytes: u64 = 0;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                text.clear();
                match e.local_name().as_ref() {
                    b"file" => {
                        let mut new = NzbEntry::default();
                        for a in e.attributes().flatten() {
                            if a.key.local_name().as_ref() == b"subject" {
                                new.subject = a
                                    .decode_and_unescape_value(reader.decoder())
                                    .map(|v| v.into_owned())
                                    .unwrap_or_default();
                            }
                        }
                        new.filename = quoted_name(&new.subject);
                        entry = Some(new);
                    }
                    b"segment" => {
                        bytes = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.local_name().as_ref() == b"bytes")
                            .and_then(|a| String::from_utf8_lossy(&a.value).parse().ok())
                            .unwrap_or(0);
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                text.push_str(&t.decode().map(|v| v.into_owned()).unwrap_or_default())
            }
            Ok(Event::GeneralRef(r)) => {
                let name = r.decode().map(|v| v.into_owned()).unwrap_or_default();
                match name.as_str() {
                    "lt" => text.push('<'),
                    "gt" => text.push('>'),
                    "amp" => text.push('&'),
                    "quot" => text.push('"'),
                    "apos" => text.push('\''),
                    _ => {
                        if let Ok(Some(c)) = r.resolve_char_ref() {
                            text.push(c);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"group" => {
                    if let Some(entry) = entry.as_mut() {
                        entry.groups.push(text.trim().to_string());
                    }
                }
                b"segment" => {
                    if let Some(entry) = entry.as_mut() {
                        entry.segments.push((bytes, message_id(text.trim())));
                    }
                }
                b"file" => {
                    if let Some(done) = entry.take() {
                        if !done.segments.is_empty() {
                            entries.push(done);
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    entries
}

fn quoted_name(subject: &str) -> String {
    let mut parts = subject.splitn(3, '"');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(name), Some(_)) => name.replace(['/', '\\'], "_"),
        _ => String::new(),
    }
}

/// It goes into NNTP commands as is (see NzbFile): no line breaks, spaces or
/// control characters, and bounded.
fn message_id(raw: &str) -> String {
    let id: String = raw
        .chars()
        .take(1000)
        .map(|c| {
            if (c as u32) <= 0x20 || c == '\x7f' {
                '_'
            } else {
                c
            }
        })
        .collect();
    format!("<{id}>")
}

/// Formats the log message emitted when a duplicate's `kind` (`"twin"`, `"twin:N/M"`,
/// or `"alt"`) against `primary_name` changes (`TwinCheck::ServiceWork` in `TwinCheck.cpp`).
pub fn format_twin_kind_message(
    nzb_name: &[u8],
    primary_name: &[u8],
    kind: &[u8],
    by_par2: bool,
) -> Vec<u8> {
    if kind.is_empty() {
        return Vec::new();
    }
    let is_twin = kind[0] == b't';
    let colon_pos = kind.iter().position(|&b| b == b':');
    let label: &[u8] = if is_twin {
        if colon_pos.is_some() {
            b"a near-twin (files the same: "
        } else {
            b"a twin"
        }
    } else {
        b"an alt"
    };
    let reason: &[u8] = if !by_par2 {
        if is_twin {
            b"the articles sampled are identical"
        } else {
            b"another encode: the articles sampled differ"
        }
    } else if is_twin {
        b"byte-identical files by their par2 checksums"
    } else {
        b"another encode: its files' par2 checksums differ"
    };

    let mut out =
        Vec::with_capacity(nzb_name.len() + primary_name.len() + label.len() + reason.len() + 32);
    out.extend_from_slice(nzb_name);
    out.extend_from_slice(b" is ");
    out.extend_from_slice(label);
    if let Some(colon) = colon_pos {
        out.extend_from_slice(&kind[colon + 1..]);
        out.push(b')');
    }
    out.extend_from_slice(b" of ");
    out.extend_from_slice(primary_name);
    out.extend_from_slice(b" (");
    out.extend_from_slice(reason);
    out.push(b')');
    out
}

/// `TwinCheckJob::IndexData` (`TwinCheck.cpp`): selects the index of the smallest `.par2`
/// entry in `entries` within `max_index_size`, returning `-1` if no eligible `.par2` entry exists.
pub fn select_min_par2_index(is_par2: &[u8], sizes: &[i64], max_index_size: i64) -> i32 {
    let n = is_par2.len().min(sizes.len());
    let mut best_idx: i32 = -1;
    let mut best_size: i64 = i64::MAX;
    for i in 0..n {
        if is_par2[i] != 0 && (best_idx < 0 || sizes[i] < best_size) {
            best_idx = i as i32;
            best_size = sizes[i];
        }
    }
    if best_idx >= 0 && best_size <= max_index_size {
        best_idx
    } else {
        -1
    }
}

/// `TwinCheckJob::Run` (`TwinCheck.cpp`): parses the trailing `-<files>` count from a
/// PAR2 fingerprint string (`"<hash>-<count>"`).
pub fn parse_fingerprint_files(fingerprint: &[u8]) -> i32 {
    if fingerprint.is_empty() {
        return 0;
    }
    let start = match fingerprint.iter().rposition(|&b| b == b'-') {
        Some(pos) => pos + 1,
        None => 0,
    };
    let bytes = &fingerprint[start..];
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < bytes.len() {
        if bytes[i] == b'-' {
            neg = true;
            i += 1;
        } else if bytes[i] == b'+' {
            i += 1;
        }
    }
    let mut acc: i64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        acc = acc.wrapping_mul(10).wrapping_add((bytes[i] - b'0') as i64);
        i += 1;
    }
    if neg {
        (-acc) as i32
    } else {
        acc as i32
    }
}

/// `TwinCheckJob::Sample` (`TwinCheck.cpp`): returns `true` when entry `a` sorts before `b`
/// by `(filename if non-empty else subject)`.
#[inline]
pub fn compare_entry_names(
    a_filename: &[u8],
    a_subject: &[u8],
    b_filename: &[u8],
    b_subject: &[u8],
) -> bool {
    let ka = if a_filename.is_empty() {
        a_subject
    } else {
        a_filename
    };
    let kb = if b_filename.is_empty() {
        b_subject
    } else {
        b_filename
    };
    ka < kb
}

/// `TwinCheckJob::Sample` (`TwinCheck.cpp`): selects `sample_count` evenly spaced `(file_idx, seg_idx)`
/// pairs across two postings when their non-PAR2 file count and per-segment byte sizes match.
pub fn sample_indices(
    primary_seg_counts: &[usize],
    primary_seg_bytes: &[i64],
    dupe_seg_counts: &[usize],
    dupe_seg_bytes: &[i64],
    sample_count: usize,
) -> Option<Vec<(usize, usize)>> {
    if primary_seg_counts.is_empty()
        || primary_seg_counts.len() != dupe_seg_counts.len()
        || sample_count == 0
    {
        return None;
    }

    let mut places = Vec::new();
    let mut p_off = 0usize;
    let mut d_off = 0usize;

    for i in 0..primary_seg_counts.len() {
        let p_cnt = primary_seg_counts[i];
        let d_cnt = dupe_seg_counts[i];
        if p_cnt != d_cnt
            || p_off + p_cnt > primary_seg_bytes.len()
            || d_off + d_cnt > dupe_seg_bytes.len()
        {
            return None;
        }
        for k in 0..p_cnt {
            if primary_seg_bytes[p_off + k] != dupe_seg_bytes[d_off + k] {
                return None;
            }
            places.push((i, k));
        }
        p_off += p_cnt;
        d_off += d_cnt;
    }

    if places.is_empty() {
        return None;
    }

    let mut sampled = Vec::with_capacity(sample_count);
    for n in 0..sample_count {
        let idx = ((2 * n + 1) * places.len()) / (2 * sample_count);
        sampled.push(places[idx]);
    }
    Some(sampled)
}

/// `TwinCheckJob::Sample` (`TwinCheck.cpp`): updates `(compared, differ)` for a sampled pair.
#[inline]
pub fn compare_sampled_pair(
    part_a_success: i32,
    part_b_success: i32,
    part_a_offset: i64,
    part_b_offset: i64,
    data_equal: i32,
    in_compared: i32,
    in_differ: i32,
) -> (i32, i32) {
    if part_a_success != 0 && part_b_success != 0 {
        let compared = in_compared.wrapping_add(1);
        let differ = if part_a_offset != part_b_offset || data_equal == 0 {
            in_differ.wrapping_add(1)
        } else {
            in_differ
        };
        (compared, differ)
    } else {
        (in_compared, in_differ)
    }
}

/// `TwinCheckJob::Sample` (`TwinCheck.cpp`): evaluates the sample verdict (`0` = retry later,
/// `1` = `"twin"`, `2` = `"alt"`).
#[inline]
pub fn eval_sample_verdict(is_stopped: bool, compared: i32, differ: i32, sample_count: i32) -> i32 {
    if is_stopped || (compared < sample_count / 2 && differ == 0) {
        0
    } else if differ > 0 {
        2
    } else {
        1
    }
}

/// `TwinCheck::Identity` (`TwinCheck.cpp`): formats `<files_param>#<posting>`,
/// or returns an empty buffer if `files_param` is empty, `"none"`, or `"lost"`.
pub fn format_identity(files_param: &[u8], posting: &[u8]) -> Vec<u8> {
    if files_param.is_empty() || files_param == b"none" || files_param == b"lost" {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(files_param.len() + 1 + posting.len());
    out.extend_from_slice(files_param);
    out.push(b'#');
    out.extend_from_slice(posting);
    out
}

/// `TwinCheck::PutPar2` (`TwinCheck.cpp`): splits a `"<print>\t<1|0>"` string returned by
/// `nzbget_rs_twin_put_par2` into `(print_slice, is_complete)`.
pub fn split_put_par2_result(raw: &[u8]) -> (&[u8], bool) {
    match raw.iter().position(|&b| b == b'\t') {
        Some(tab) => (&raw[..tab], &raw[tab + 1..] == b"1"),
        None => (raw, false),
    }
}

/// `TwinCheck::FetchEntry` (`TwinCheck.cpp`): validates a fetched segment part and computes
/// the required vector size `(is_valid, new_len)`.
#[inline]
pub fn is_fetched_entry_part_valid(
    success: bool,
    offset: i64,
    data_len: usize,
    max_size: i64,
    cur_data_len: usize,
) -> (bool, usize) {
    if !success || offset < 0 || offset.saturating_add(data_len as i64) > max_size {
        return (false, cur_data_len);
    }
    let end_len = (offset as usize).saturating_add(data_len);
    let new_len = cur_data_len.max(end_len);
    (true, new_len)
}

/// `TwinCheck::ServiceWork` (`TwinCheck.cpp`): evaluates whether an item is a duplicate.
#[inline]
pub fn is_dupe_item(
    queued: bool,
    remaining_size: i64,
    paused_size: i64,
    delete_status: i32,
    ds_dupe: i32,
    ds_copy: i32,
) -> bool {
    if queued {
        remaining_size > 0 && paused_size >= remaining_size
    } else {
        delete_status == ds_dupe || delete_status == ds_copy
    }
}

/// `TwinCheck::ServiceWork` (`TwinCheck.cpp`): returns `true` when `print` is non-empty and
/// neither `"none"` nor `"lost"`.
#[inline]
pub fn is_print_usable(print: &[u8]) -> bool {
    !print.is_empty() && print != b"none" && print != b"lost"
}

/// `TwinCheck::ServiceWork` (`TwinCheck.cpp`): selects the primary download index within a `dupe_key` group.
pub fn select_primary(queued: &[u8], is_dupe: &[u8], scores: &[i32]) -> i32 {
    let n = queued.len().min(is_dupe.len()).min(scores.len());
    let mut primary: i32 = -1;

    for i in 0..n {
        if queued[i] != 0
            && is_dupe[i] == 0
            && (primary < 0 || scores[i] > scores[primary as usize])
        {
            primary = i as i32;
        }
    }
    if primary < 0 {
        for i in 0..n {
            if queued[i] == 0 && is_dupe[i] == 0 {
                primary = i as i32;
                break;
            }
        }
    }
    if primary < 0 {
        for i in 0..n {
            if queued[i] != 0 && (primary < 0 || scores[i] > scores[primary as usize]) {
                primary = i as i32;
            }
        }
    }

    primary
}

/// `TwinCheck::ServiceWork` (`TwinCheck.cpp`): evaluates `(sampleable, sampled_now)`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn eval_sampleable(
    primary_ok: bool,
    is_primary: bool,
    is_dupe: bool,
    print_empty: bool,
    primary_print_empty: bool,
    by_par2: bool,
    has_sampled: bool,
    sampled_of: Option<&[u8]>,
    primary_id: &[u8],
) -> (bool, bool) {
    let sampleable =
        primary_ok && !is_primary && is_dupe && !print_empty && !primary_print_empty && !by_par2;
    let sampled_now = has_sampled && sampled_of.is_some_and(|s| s == primary_id);
    (sampleable, sampled_now)
}

/// `TwinCheck::ServiceWork` (`TwinCheck.cpp`): resolves the `"twin"`, `"twin:N/M"`, `"alt"`, or `""`
/// kind label for a duplicate item.
#[allow(clippy::too_many_arguments)]
pub fn resolve_kind(
    is_primary: bool,
    is_dupe: bool,
    by_par2: bool,
    by_files: &[u8],
    listed: bool,
    same_print: bool,
    sampleable: bool,
    sampled_now: bool,
    sampled_val: &[u8],
) -> Vec<u8> {
    if is_primary || !is_dupe {
        return Vec::new();
    }
    if by_par2 {
        if !by_files.is_empty() || listed {
            return by_files.to_vec();
        } else if same_print {
            return b"twin".to_vec();
        } else {
            return b"alt".to_vec();
        }
    }
    if sampleable && sampled_now {
        if sampled_val == b"twin" {
            return b"twin".to_vec();
        } else if sampled_val == b"alt" {
            return b"alt".to_vec();
        }
    }
    Vec::new()
}

// ============================================================================
// Native TwinCheck Coordinator & TwinCheckJob Orchestration
// ============================================================================

pub const SAMPLE_COUNT: usize = 6;
pub const MAX_INDEX_SIZE: i64 = 16 * 1024 * 1024;
pub const MAX_RUNNING: usize = 2;
pub const MAX_ATTEMPTS: i32 = 3;

#[cfg(unix)]
#[derive(Clone)]
struct ScheduledTwinJob {
    nzb_id: i32,
    nzb_filename: Vec<u8>,
    hinted: bool,
    primary_id: i32,
    primary_nzb_filename: Vec<u8>,
    posting: Vec<u8>,
    rank: i32,
}

struct ActiveTwinJob {
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    fetcher: std::sync::Arc<crate::articlefetcher::ArticleFetcher>,
    #[cfg(unix)]
    spec: ScheduledTwinJob,
}

#[derive(Default)]
struct TwinCoordState {
    stopping: bool,
    attached: bool,
    jobs: std::collections::BTreeMap<usize, ActiveTwinJob>,
    checking: std::collections::BTreeSet<i32>,
    failed_attempts: std::collections::BTreeMap<i32, i32>,
}

static OPENED_STORE_PATH: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static TWIN_COORD: std::sync::OnceLock<Mutex<TwinCoordState>> = std::sync::OnceLock::new();
#[cfg(unix)]
static NEXT_TWIN_JOB_TOKEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
static TWIN_JOB_COUNT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
#[cfg(unix)]
static TWIN_NUDGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(unix)]
static TWIN_LAST_PRUNE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

fn twin_coord() -> &'static Mutex<TwinCoordState> {
    TWIN_COORD.get_or_init(|| Mutex::new(TwinCoordState::default()))
}

#[inline]
fn decode_utf8_chunks(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_owned();
    }
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            out.push('\u{FFFD}');
        }
    }
    out
}

#[cfg(unix)]
#[inline]
fn lower_dupe_key_bytes(raw: &[u8]) -> Vec<u8> {
    raw.iter()
        .map(|&b| unsafe { libc::tolower(std::ffi::c_int::from(b)) as u8 })
        .collect()
}

#[cfg(unix)]
fn twin_file_exists(path: &[u8]) -> bool {
    if path.is_empty() {
        return false;
    }
    crate::fsops::file_exists(path, &crate::fsops::posix_stat)
}

pub(crate) fn ensure_store_open_dir(qdir: &[u8]) -> bool {
    if qdir.is_empty() {
        return false;
    }
    let mut full_path = Vec::with_capacity(qdir.len() + 12);
    full_path.extend_from_slice(qdir);
    full_path.push(crate::paths::PATH_SEPARATOR);
    full_path.extend_from_slice(b"twincheck");

    let mut opened = OPENED_STORE_PATH.lock().unwrap_or_else(|e| e.into_inner());
    if *opened != full_path {
        let store = Store::open(crate::fsops::path(&full_path).to_path_buf());
        *STORE.lock().unwrap_or_else(|e| e.into_inner()) = Some(store);
        *opened = full_path;
    }
    true
}

pub(crate) fn store_kind_dir(
    qdir: &[u8],
    primary_id: i32,
    primary_print: &[u8],
    dupe_id: i32,
    dupe_print: &[u8],
) -> Vec<u8> {
    if !ensure_store_open_dir(qdir) {
        return Vec::new();
    }
    let pp = decode_utf8_chunks(primary_print);
    let dp = decode_utf8_chunks(dupe_print);
    let guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(store) = guard.as_ref() else {
        return Vec::new();
    };
    kind(store.get(primary_id, &pp), store.get(dupe_id, &dp)).into_bytes()
}

pub(crate) fn store_match_dir(
    qdir: &[u8],
    own_id: i32,
    own_print: &[u8],
    target: &[u8],
    target_alt: &[u8],
    donor_id: i32,
    donor_print: &[u8],
) -> Vec<u8> {
    if !ensure_store_open_dir(qdir) {
        return Vec::new();
    }
    let op = decode_utf8_chunks(own_print);
    let dp = decode_utf8_chunks(donor_print);
    let tgt = decode_utf8_chunks(target);
    let alt = decode_utf8_chunks(target_alt);
    let found = {
        let guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().and_then(|store| {
            match_by_content(store.get(own_id, &op), &tgt, &alt, store.get(donor_id, &dp))
        })
    };
    match found {
        None => Vec::new(),
        Some(name) if name.is_empty() => b"-".to_vec(),
        Some(name) => name.into_bytes(),
    }
}

pub(crate) fn reset_all() {
    let mut guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
    guard.stopping = false;
}

pub(crate) fn stop_all_jobs() {
    let mut guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
    guard.stopping = true;
    for job in guard.jobs.values() {
        job.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
        job.fetcher.stop();
    }
}

pub(crate) fn wait_all() {
    while TWIN_JOB_COUNT.load(std::sync::atomic::Ordering::SeqCst) > 0 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(unix)]
pub(crate) fn create_daemon_twincheck() {
    let mut guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
    guard.stopping = false;
    guard.attached = true;
}

#[cfg(unix)]
pub(crate) fn shutdown_daemon_twincheck() {
    let mut guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
    guard.attached = false;
}

#[cfg(unix)]
pub(crate) fn nudge_and_wake_active() {
    let attached = twin_coord().lock().map_or(false, |g| g.attached);
    if attached {
        TWIN_NUDGED.store(true, std::sync::atomic::Ordering::SeqCst);
        crate::scheduler::wake_active_service_coord();
    }
}

#[cfg(unix)]
pub(crate) fn service_interval_active() -> i32 {
    if TWIN_NUDGED.load(std::sync::atomic::Ordering::SeqCst) {
        1000
    } else {
        15000
    }
}

#[cfg(unix)]
fn store_has_sigs_dir(qdir: &[u8], id: i32, print: &[u8]) -> i32 {
    if !ensure_store_open_dir(qdir) {
        return 0;
    }
    let print_s = decode_utf8_chunks(print);
    let guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(store) = guard.as_ref() else {
        return 0;
    };
    let listed = store.get(id, &print_s);
    if listed.sigs.is_empty() {
        0
    } else if listed.complete {
        2
    } else {
        1
    }
}

#[cfg(unix)]
fn store_put_par2_dir(qdir: &[u8], id: i32, posting: &[u8], data: &[u8]) -> (String, bool) {
    if data.is_empty() || !ensure_store_open_dir(qdir) {
        return (String::new(), false);
    }
    let (mut sigs, complete) = par2_set(data);
    let posting_s = decode_utf8_chunks(posting);
    if !complete {
        let earlier = {
            let guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
            guard
                .as_ref()
                .and_then(|store| store.partial_of(id, &posting_s).map(|l| l.sigs.clone()))
        };
        if let Some(prev) = earlier {
            sigs = merge(&prev, &sigs);
        }
    }
    let print = fingerprint(&sigs);
    if !sigs.is_empty() {
        let identity = format!("{print}#{posting_s}");
        let listed = Listed {
            sigs,
            complete,
            print: identity,
        };
        let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(store) = guard.as_mut() {
            store.put(id, listed);
        }
    }
    (print, complete)
}

#[cfg(unix)]
fn store_prune_dir(qdir: &[u8], live: &[(i32, String)]) {
    if !ensure_store_open_dir(qdir) {
        return;
    }
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(store) = guard.as_mut() {
        store.prune(live);
    }
}

#[cfg(unix)]
fn twin_attempts(nzb_id: i32) -> i32 {
    let guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
    guard.failed_attempts.get(&nzb_id).copied().unwrap_or(0)
}

#[cfg(unix)]
struct TwinQueueItem {
    nzb_ptr: usize,
    id: i32,
    queued: bool,
    rank: i32,
    is_dupe: bool,
    dupe_score: i32,
    hinted: bool,
    posting: Vec<u8>,
    name: Vec<u8>,
    queued_filename: Vec<u8>,
    files_param: Vec<u8>,
    has_sampled: bool,
    sampled_param: Vec<u8>,
    has_sampled_of: bool,
    sampled_of_param: Vec<u8>,
    kind_param: Vec<u8>,
}

#[cfg(unix)]
struct TwinLockedWorkCtx {
    prune: bool,
    cur_time: i64,
    qdir: Vec<u8>,
    prune_ran: bool,
    live: Vec<(i32, String)>,
    jobs: Vec<ScheduledTwinJob>,
}

#[cfg(unix)]
fn run_service_work_locked(
    qh: &crate::queuesubsys_host::QueueSubsysHost,
    ctx: &mut TwinLockedWorkCtx,
    dq_ptr: usize,
) {
    let queue_len = qh.queue_len(dq_ptr);
    let history_len = qh.history_len(dq_ptr);
    let ds_dupe = crate::queuesubsys_host::DS_DUPE;
    let ds_copy = crate::queuesubsys_host::DS_COPY;

    if ctx.prune {
        TWIN_LAST_PRUNE.store(ctx.cur_time, std::sync::atomic::Ordering::SeqCst);
        ctx.prune_ran = true;
    }

    let mut keys: std::collections::BTreeMap<Vec<u8>, Vec<TwinQueueItem>> =
        std::collections::BTreeMap::new();

    let mut fetch_item = |nzb_ptr: usize, queued: bool, rank: i32| {
        if nzb_ptr == 0 {
            return;
        }
        let snap = qh.nzb_snap(nzb_ptr, false);
        let id = snap.id;
        let posting = snap.full_content_hash.to_string().into_bytes();
        let files_param = qh.nzb_get_param(nzb_ptr, b"DupeFiles").unwrap_or_default();
        if ctx.prune {
            let ident = format_identity(&files_param, &posting);
            if !ident.is_empty() {
                ctx.live.push((id, decode_utf8_chunks(&ident)));
            }
        }
        if snap.kind != crate::queuesubsys_host::NK_NZB {
            return;
        }
        let key = lower_dupe_key_bytes(&snap.dupe_key);
        if key.is_empty() {
            return;
        }
        let is_dupe = is_dupe_item(
            queued,
            snap.remaining_size,
            snap.paused_size,
            snap.delete_status,
            ds_dupe,
            ds_copy,
        );
        let hinted = qh.nzb_get_param(nzb_ptr, b"DupeSameSize").is_some()
            || qh.nzb_get_param(nzb_ptr, b"DupeSameInner").is_some();
        let p_sampled = qh.nzb_get_param(nzb_ptr, b"DupeSampled");
        let p_sampled_of = qh.nzb_get_param(nzb_ptr, b"DupeSampledOf");
        let kind_param = qh.nzb_get_param(nzb_ptr, b"DupeKind").unwrap_or_default();
        let item = TwinQueueItem {
            nzb_ptr,
            id,
            queued,
            rank,
            is_dupe,
            dupe_score: snap.dupe_score,
            hinted,
            posting,
            name: snap.name,
            queued_filename: snap.queued_filename,
            files_param,
            has_sampled: p_sampled.is_some(),
            sampled_param: p_sampled.unwrap_or_default(),
            has_sampled_of: p_sampled_of.is_some(),
            sampled_of_param: p_sampled_of.unwrap_or_default(),
            kind_param,
        };
        keys.entry(key).or_default().push(item);
    };

    for idx in 0..queue_len {
        fetch_item(qh.queue_at(dq_ptr, idx), true, 0);
    }
    for idx in 0..history_len {
        let hi = qh.history_at(dq_ptr, idx);
        if hi.kind == crate::queuesubsys_host::HK_NZB {
            fetch_item(hi.inner_ptr, false, (idx + 1) as i32);
        }
    }

    let mut changed = false;

    for (_key, mut items) in keys {
        items.retain(|it| !it.queued_filename.contains(&b'|'));
        if items.len() < 2 {
            continue;
        }
        let first_job = ctx.jobs.len();
        let key_rank = items.iter().map(|it| it.rank).min().unwrap_or(i32::MAX);

        for item in &items {
            let ident = format_identity(&item.files_param, &item.posting);
            let listless = is_print_usable(&item.files_param)
                && store_has_sigs_dir(&ctx.qdir, item.id, &ident) < 2
                && twin_attempts(item.id) < MAX_ATTEMPTS;
            let is_merged = item.queued_filename.contains(&b'|');
            if (item.files_param.is_empty() || listless)
                && !is_merged
                && twin_file_exists(&item.queued_filename)
            {
                ctx.jobs.push(ScheduledTwinJob {
                    nzb_id: item.id,
                    nzb_filename: item.queued_filename.clone(),
                    hinted: item.hinted,
                    primary_id: 0,
                    primary_nzb_filename: Vec::new(),
                    posting: item.posting.clone(),
                    rank: 0,
                });
            }
        }

        let item_queued: Vec<u8> = items.iter().map(|it| u8::from(it.queued)).collect();
        let item_dupe: Vec<u8> = items.iter().map(|it| u8::from(it.is_dupe)).collect();
        let item_scores: Vec<i32> = items.iter().map(|it| it.dupe_score).collect();
        let primary_idx = select_primary(&item_queued, &item_dupe, &item_scores);
        let primary_usize = if primary_idx >= 0 && (primary_idx as usize) < items.len() {
            Some(primary_idx as usize)
        } else {
            None
        };

        let primary_print: Vec<u8> = primary_usize
            .map(|pi| items[pi].files_param.clone())
            .unwrap_or_default();
        let primary_ok = primary_usize.is_some_and(|pi| {
            !items[pi].queued_filename.contains(&b'|')
                && twin_file_exists(&items[pi].queued_filename)
        });
        let primary_id_bytes: Vec<u8> = primary_usize
            .map(|pi| items[pi].id.to_string().into_bytes())
            .unwrap_or_default();
        let primary_ident: Vec<u8> = primary_usize
            .map(|pi| format_identity(&items[pi].files_param, &items[pi].posting))
            .unwrap_or_default();

        for (idx, item) in items.iter().enumerate() {
            let is_primary = primary_usize == Some(idx);
            let by_par2 = is_print_usable(&primary_print) && is_print_usable(&item.files_param);
            let (sampleable, sampled_now) = eval_sampleable(
                primary_ok,
                is_primary,
                item.is_dupe,
                item.files_param.is_empty(),
                primary_print.is_empty(),
                by_par2,
                item.has_sampled,
                item.has_sampled_of
                    .then_some(item.sampled_of_param.as_slice()),
                &primary_id_bytes,
            );
            let is_merged = item.queued_filename.contains(&b'|');
            if sampleable && !sampled_now && !is_merged && twin_file_exists(&item.queued_filename) {
                if let Some(pi) = primary_usize {
                    ctx.jobs.push(ScheduledTwinJob {
                        nzb_id: item.id,
                        nzb_filename: item.queued_filename.clone(),
                        hinted: false,
                        primary_id: items[pi].id,
                        primary_nzb_filename: items[pi].queued_filename.clone(),
                        posting: Vec::new(),
                        rank: 0,
                    });
                }
            }

            let mut by_files = Vec::new();
            let mut listed = false;
            if !is_primary && item.is_dupe && by_par2 {
                if let Some(pi) = primary_usize {
                    let item_ident = format_identity(&item.files_param, &item.posting);
                    by_files = store_kind_dir(
                        &ctx.qdir,
                        items[pi].id,
                        &primary_ident,
                        item.id,
                        &item_ident,
                    );
                    listed = store_has_sigs_dir(&ctx.qdir, items[pi].id, &primary_ident) > 0
                        || store_has_sigs_dir(&ctx.qdir, item.id, &item_ident) > 0;
                }
            }

            let resolved_kind = resolve_kind(
                is_primary,
                item.is_dupe,
                by_par2,
                &by_files,
                listed,
                item.files_param == primary_print,
                sampleable,
                sampled_now && item.has_sampled,
                &item.sampled_param,
            );
            if item.kind_param != resolved_kind {
                qh.nzb_set_param(item.nzb_ptr, b"DupeKind", Some(&resolved_kind));
                if !resolved_kind.is_empty() {
                    let p_name = primary_usize
                        .map(|pi| items[pi].name.as_slice())
                        .unwrap_or(&[]);
                    let msg = format_twin_kind_message(&item.name, p_name, &resolved_kind, by_par2);
                    qh.nzb_print_message(item.nzb_ptr, crate::queuesubsys_host::MK_INFO, &msg);
                }
                changed = true;
            }
        }

        for j in &mut ctx.jobs[first_job..] {
            j.rank = key_rank;
        }
    }

    if changed {
        qh.dq_history_changed(dq_ptr);
        qh.dq_save(dq_ptr);
    }
}

#[cfg(unix)]
pub(crate) fn service_work_active() {
    TWIN_NUDGED.store(false, std::sync::atomic::Ordering::SeqCst);
    let Some(qh) = crate::ffi::queue_host() else {
        return;
    };
    let gopts = qh.opts();
    if !gopts.dupe_check {
        return;
    }
    let cur_time = gopts.cur_time;
    let qdir = gopts.queue_dir;
    let last_prune = TWIN_LAST_PRUNE.load(std::sync::atomic::Ordering::SeqCst);
    let prune = cur_time - last_prune >= 60 * 60
        && TWIN_JOB_COUNT.load(std::sync::atomic::Ordering::SeqCst) == 0;

    let mut work_ctx = TwinLockedWorkCtx {
        prune,
        cur_time,
        qdir: qdir.clone(),
        prune_ran: false,
        live: Vec::new(),
        jobs: Vec::new(),
    };
    qh.with_locked_queue(|dq_ptr| {
        run_service_work_locked(&qh, &mut work_ctx, dq_ptr);
    });

    if work_ctx.prune_ran {
        let can_prune = {
            let guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
            if guard.jobs.is_empty() {
                true
            } else {
                TWIN_LAST_PRUNE.store(0, std::sync::atomic::Ordering::SeqCst);
                false
            }
        };
        if can_prune {
            store_prune_dir(&qdir, &work_ctx.live);
        }
    }

    work_ctx.jobs.sort_by(|a, b| {
        if a.rank != b.rank {
            a.rank.cmp(&b.rank)
        } else {
            b.hinted.cmp(&a.hinted)
        }
    });

    let mut to_spawn = Vec::new();
    {
        let mut guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
        for job in work_ctx.jobs {
            if guard.stopping || guard.jobs.len() >= MAX_RUNNING {
                break;
            }
            if guard.checking.contains(&job.nzb_id) {
                continue;
            }
            let token = NEXT_TWIN_JOB_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let fetcher = std::sync::Arc::new(crate::articlefetcher::ArticleFetcher::new(
                crate::articlefetcher::native_nntp_fetch_host,
            ));
            let nzb_id = job.nzb_id;
            guard.jobs.insert(
                token,
                ActiveTwinJob {
                    stopped: std::sync::Arc::clone(&stopped),
                    fetcher,
                    spec: job,
                },
            );
            guard.checking.insert(nzb_id);
            TWIN_JOB_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            to_spawn.push((token, stopped));
        }
    }

    for (token, stopped) in to_spawn {
        let qd = qdir.clone();
        if std::thread::Builder::new()
            .name("twin-check".into())
            .spawn(move || {
                let _tg = crate::ffi::RustThreadGuard::enter(&stopped);
                run_twin_job(token, &qd);
            })
            .is_err()
        {
            let mut guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
            if let Some(removed) = guard.jobs.remove(&token) {
                guard.checking.remove(&removed.spec.nzb_id);
                TWIN_JOB_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }
}

#[cfg(unix)]
fn read_nzb_entries_from_disk(path: &[u8]) -> Vec<NzbEntry> {
    match std::fs::read(crate::fsops::path(path)) {
        Ok(xml) => read_nzb(&xml),
        Err(_) => Vec::new(),
    }
}

#[cfg(unix)]
fn find_nzb_in_queue_or_history(
    qh: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_id: i32,
) -> usize {
    let q = qh.queue_find_by_id(dq_ptr, nzb_id);
    if q != 0 {
        return q;
    }
    let hlen = qh.history_len(dq_ptr);
    for idx in 0..hlen {
        let hi = qh.history_at(dq_ptr, idx);
        if hi.kind == crate::queuesubsys_host::HK_NZB
            && hi.inner_ptr != 0
            && qh.nzb_snap(hi.inner_ptr, false).id == nzb_id
        {
            return hi.inner_ptr;
        }
    }
    0
}

#[cfg(unix)]
fn run_twin_job(token: usize, qdir: &[u8]) {
    let (spec, fetcher, stopped) = {
        let guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
        let Some(active) = guard.jobs.get(&token) else {
            return;
        };
        (
            active.spec.clone(),
            std::sync::Arc::clone(&active.fetcher),
            std::sync::Arc::clone(&active.stopped),
        )
    };

    struct UnregisterGuard {
        token: usize,
        nzb_id: i32,
        stored: bool,
    }
    impl Drop for UnregisterGuard {
        fn drop(&mut self) {
            let wake = {
                let mut guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
                guard.jobs.remove(&self.token);
                guard.checking.remove(&self.nzb_id);
                TWIN_JOB_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                if !guard.stopping && self.stored && guard.attached {
                    TWIN_NUDGED.store(true, std::sync::atomic::Ordering::SeqCst);
                    true
                } else {
                    false
                }
            };
            if wake {
                crate::scheduler::wake_active_service_coord();
            }
        }
    }

    let mut unreg = UnregisterGuard {
        token,
        nzb_id: spec.nzb_id,
        stored: false,
    };

    let is_stopped =
        || -> bool { fetcher.is_stopped() || stopped.load(std::sync::atomic::Ordering::SeqCst) };

    if spec.primary_id != 0 {
        let load_data_files = |nzb_path: &[u8]| -> Vec<NzbEntry> {
            let mut files: Vec<NzbEntry> = read_nzb_entries_from_disk(nzb_path)
                .into_iter()
                .filter(|e| !e.is_par2())
                .collect();
            files.sort_by(|a, b| {
                if compare_entry_names(
                    a.filename.as_bytes(),
                    a.subject.as_bytes(),
                    b.filename.as_bytes(),
                    b.subject.as_bytes(),
                ) {
                    std::cmp::Ordering::Less
                } else if compare_entry_names(
                    b.filename.as_bytes(),
                    b.subject.as_bytes(),
                    a.filename.as_bytes(),
                    a.subject.as_bytes(),
                ) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            files
        };

        let primary = load_data_files(&spec.primary_nzb_filename);
        let dupe = load_data_files(&spec.nzb_filename);
        let primary_id_bytes = spec.primary_id.to_string().into_bytes();

        let flatten = |entries: &[NzbEntry]| -> (Vec<usize>, Vec<i64>) {
            let mut counts = Vec::with_capacity(entries.len());
            let mut bytes = Vec::new();
            for e in entries {
                counts.push(e.segments.len());
                for &(b, _) in &e.segments {
                    bytes.push(b as i64);
                }
            }
            (counts, bytes)
        };
        let (p_counts, p_bytes) = flatten(&primary);
        let (d_counts, d_bytes) = flatten(&dupe);

        let store_sample = |val: &[u8]| -> bool {
            let Some(qh) = crate::ffi::queue_host() else {
                return false;
            };
            qh.with_locked_queue(|dq_ptr| {
                let nzb_ptr = find_nzb_in_queue_or_history(&qh, dq_ptr, spec.nzb_id);
                if nzb_ptr == 0 {
                    return false;
                }
                qh.nzb_set_param(nzb_ptr, b"DupeSampled", Some(val));
                qh.nzb_set_param(nzb_ptr, b"DupeSampledOf", Some(&primary_id_bytes));
                qh.dq_history_changed(dq_ptr);
                qh.dq_save(dq_ptr);
                true
            })
        };

        let Some(pairs) = sample_indices(&p_counts, &p_bytes, &d_counts, &d_bytes, SAMPLE_COUNT)
        else {
            if store_sample(b"none") {
                unreg.stored = true;
            }
            return;
        };

        let mut compared = 0i32;
        let mut differ = 0i32;
        for (file_idx, seg_idx) in pairs {
            if is_stopped() {
                break;
            }
            let a = &primary[file_idx];
            let b = &dupe[file_idx];
            let a_groups: Vec<Vec<u8>> = a.groups.iter().map(|g| g.as_bytes().to_vec()).collect();
            let b_groups: Vec<Vec<u8>> = b.groups.iter().map(|g| g.as_bytes().to_vec()).collect();
            let part_a = fetcher.fetch(a.segments[seg_idx].1.as_bytes(), &a_groups);
            let part_b = fetcher.fetch(b.segments[seg_idx].1.as_bytes(), &b_groups);
            let (c_next, d_next) = compare_sampled_pair(
                i32::from(part_a.success),
                i32::from(part_b.success),
                part_a.offset,
                part_b.offset,
                i32::from(part_a.data == part_b.data),
                compared,
                differ,
            );
            compared = c_next;
            differ = d_next;
        }

        let verdict = eval_sample_verdict(is_stopped(), compared, differ, SAMPLE_COUNT as i32);
        if verdict == 0 {
            return;
        }
        let sampled_val: &[u8] = if verdict == 2 { b"alt" } else { b"twin" };
        if store_sample(sampled_val) {
            unreg.stored = true;
        }
        return;
    }

    let entries = read_nzb_entries_from_disk(&spec.nzb_filename);
    let is_par2: Vec<u8> = entries.iter().map(|e| u8::from(e.is_par2())).collect();
    let sizes: Vec<i64> = entries.iter().map(|e| e.size() as i64).collect();
    let best_idx = select_min_par2_index(&is_par2, &sizes, MAX_INDEX_SIZE);
    let (data_opt, fetched) = if best_idx < 0 {
        (None, false)
    } else {
        (
            crate::articlefetcher::fetch_entry(
                &fetcher,
                &entries[best_idx as usize],
                MAX_INDEX_SIZE,
                is_stopped,
            ),
            true,
        )
    };
    if is_stopped() {
        return;
    }
    let (mut fp, complete) = match data_opt {
        Some(ref data) => store_put_par2_dir(qdir, spec.nzb_id, &spec.posting, data),
        None => (String::new(), false),
    };
    let known = !fp.is_empty();
    if fetched && (!known || !complete) {
        let mut guard = twin_coord().lock().unwrap_or_else(|e| e.into_inner());
        let att = guard.failed_attempts.entry(spec.nzb_id).or_insert(0);
        *att += 1;
        if *att < MAX_ATTEMPTS {
            return;
        }
    }
    if !known {
        fp = if fetched {
            "lost".to_string()
        } else {
            "none".to_string()
        };
    }
    let files = if known {
        parse_fingerprint_files(fp.as_bytes())
    } else {
        0
    };
    let stored_ok = if let Some(qh) = crate::ffi::queue_host() {
        qh.with_locked_queue(|dq_ptr| {
            let nzb_ptr = find_nzb_in_queue_or_history(&qh, dq_ptr, spec.nzb_id);
            if nzb_ptr == 0 {
                return false;
            }
            let name = qh.nzb_snap(nzb_ptr, false).name;
            qh.nzb_set_param(nzb_ptr, b"DupeFiles", Some(fp.as_bytes()));
            let mut msg = b"Fingerprint of ".to_vec();
            msg.extend_from_slice(&name);
            msg.extend_from_slice(format!(": {fp} ({files} file(s) in its par2 set)").as_bytes());
            qh.nzb_print_message(nzb_ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
            qh.dq_history_changed(dq_ptr);
            qh.dq_save(dq_ptr);
            true
        })
    } else {
        false
    };
    if stored_ok {
        unreg.stored = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn sig(name: &str, md5: &str, length: u64) -> FileSig {
        FileSig {
            name: name.into(),
            md5: md5.into(),
            length,
        }
    }

    #[test]
    fn fingerprint_matches_the_cpp_values() {
        // ignores names and order, counts distinct files
        let a = vec![sig("x.part01.rar", "aa", 10), sig("x.part02.rar", "bb", 20)];
        let b = vec![sig("RANDOM2", "bb", 20), sig("RANDOM1", "aa", 10)];
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert!(fingerprint(&a).ends_with("-2"));
        assert_eq!(fingerprint(&[]), "");
        // FNV-1a of "aa:10\n" then "bb:20\n"
        let mut hash: u64 = 14695981039346656037;
        for byte in "aa:10\nbb:20\n".bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(1099511628211);
        }
        assert_eq!(fingerprint(&a), format!("{hash:016x}-2"));
    }

    #[test]
    fn volumes() {
        assert_eq!(volume_blocks("x.vol03+04.par2"), Some(4));
        assert_eq!(volume_blocks("X.VOL00+128.PAR2"), Some(128));
        assert_eq!(volume_blocks("x.vol-08.par2"), None);
        assert_eq!(volume_blocks("x.par2"), None);
    }

    fn listed(sigs: Vec<FileSig>, complete: bool) -> Listed {
        Listed {
            print: fingerprint(&sigs),
            sigs,
            complete,
        }
    }

    #[test]
    fn kinds() {
        let primary = listed(
            vec![sig("a", "1", 1), sig("b", "2", 2), sig("c", "3", 3)],
            true,
        );
        assert_eq!(kind(&primary, &primary), "twin");
        // two of three files: a near-twin
        let near = listed(
            vec![sig("x", "1", 1), sig("y", "2", 2), sig("z", "9", 3)],
            true,
        );
        assert_eq!(kind(&primary, &near), "twin:2/3");
        // one shared file (an NFO) of three: an alt, not a near-twin
        let nfo = listed(
            vec![sig("x", "1", 1), sig("y", "8", 2), sig("z", "9", 3)],
            true,
        );
        assert_eq!(kind(&primary, &nfo), "alt");
        // ... unless a list is incomplete: then it can't tell
        assert_eq!(kind(&primary, &listed(nfo.sigs.clone(), false)), "");
        assert_eq!(kind(&primary, &listed(vec![], true)), "");
        // two partial lists of the same first files: not a twin, not known
        let part = listed(vec![sig("a", "1", 1)], false);
        assert_eq!(kind(&part, &part), "");
        // a partial primary list gives no ratio
        assert_eq!(kind(&listed(primary.sigs.clone(), false), &near), "");
        // a complete primary, the donor's list partial: a near-twin still shows
        assert_eq!(
            kind(&primary, &listed(near.sigs.clone(), false)),
            "twin:2/3"
        );
        assert_eq!(
            kind(&primary, &listed(primary.sigs.clone(), false)),
            "twin:3/3"
        );
    }

    #[test]
    fn identical_files_count_once() {
        // two identical files in the primary; the dupe holds one of them and an
        // unrelated file: one shared, not two (no exact twin)
        let primary = listed(vec![sig("a", "1", 1), sig("b", "1", 1)], true);
        let dupe = listed(vec![sig("x", "1", 1), sig("y", "7", 7)], true);
        assert_eq!(shared_files(&primary.sigs, &dupe.sigs), 1);
        assert_eq!(kind(&primary, &dupe), "alt");
    }

    #[test]
    fn content_match() {
        let own = listed(
            vec![
                sig("QnL.7z.002", "80a3", 500),
                sig("QnL.7z.001", "9f9b", 500),
            ],
            true,
        );
        let donor = listed(
            vec![
                sig("TA7.7z.001", "d512", 500),
                sig("TA7.7z.002", "80a3", 500),
            ],
            true,
        );
        assert_eq!(
            match_by_content(&own, "QnL.7z.002", "", &donor),
            Some("TA7.7z.002".into())
        );
        // a complete donor list without the bytes: authoritative
        assert_eq!(
            match_by_content(&own, "QnL.7z.001", "", &donor),
            Some(String::new())
        );
        // case differs, one candidate: found
        assert_eq!(
            match_by_content(&own, "qnl.7Z.002", "", &donor),
            Some("TA7.7z.002".into())
        );
        assert_eq!(match_by_content(&own, "other", "", &donor), None);
        assert_eq!(
            match_by_content(&own, "other", "QnL.7z.002", &donor),
            Some("TA7.7z.002".into())
        );
        // an incomplete donor list without the bytes: not known
        let partial = listed(vec![sig("TA7.7z.001", "d512", 500)], false);
        assert_eq!(match_by_content(&own, "QnL.7z.002", "", &partial), None);
        // no donor list (not fingerprinted yet): not known
        assert_eq!(
            match_by_content(&own, "QnL.7z.002", "", &listed(vec![], false)),
            None
        );
        // two donor files hold the bytes: left to the structure
        let twice = listed(
            vec![sig("P.001", "80a3", 500), sig("P.002", "80a3", 500)],
            true,
        );
        assert_eq!(match_by_content(&own, "QnL.7z.002", "", &twice), None);
    }

    #[test]
    fn par2_names_made_valid() {
        // the names the queue gives files: a PAR2 name with a separator or a colon
        assert_eq!(valid_name("dir/file.rar"), "dir_file.rar");
        assert_eq!(valid_name("a:b.rar"), "a_b.rar");
        // matched by the valid name, the PAR2 name kept as it is
        let own = listed(vec![sig("a:b.rar", "1", 1)], true);
        let donor = listed(vec![sig("x?y.rar", "1", 1)], true);
        assert_eq!(
            match_by_content(&own, "a_b.rar", "", &donor),
            Some("x_y.rar".into())
        );
        assert_eq!(own.sigs[0].name, "a:b.rar");
    }

    #[test]
    fn partial_own_list_matches_nothing() {
        let own = listed(vec![sig("a?b.rar", "2", 1)], false);
        let donor = listed(vec![sig("y", "2", 1)], true);
        assert_eq!(match_by_content(&own, "a_b.rar", "", &donor), None);
    }

    #[test]
    fn partial_lists_merge() {
        let first = vec![sig("a", "1", 1), sig("b", "2", 2)];
        let third = vec![sig("b", "2", 2)];
        assert_eq!(merge(&first, &third), first);
        assert_eq!(
            merge(&third, &[sig("c", "3", 3)]),
            vec![sig("b", "2", 2), sig("c", "3", 3)]
        );
    }

    #[test]
    fn colliding_valid_names_are_open() {
        let own = listed(vec![sig("a:b.rar", "1", 1), sig("a?b.rar", "2", 1)], true);
        let donor = listed(vec![sig("x", "1", 1), sig("y", "2", 1)], true);
        assert_eq!(match_by_content(&own, "a_b.rar", "", &donor), None);
    }

    #[test]
    fn exact_case_first() {
        let own = listed(vec![sig("A.rar", "1", 1), sig("a.rar", "2", 1)], true);
        let donor = listed(vec![sig("x", "1", 1), sig("y", "2", 1)], true);
        assert_eq!(
            match_by_content(&own, "a.rar", "", &donor),
            Some("y".into())
        );
        assert_eq!(
            match_by_content(&own, "A.rar", "", &donor),
            Some("x".into())
        );
        // folded only, two candidates: not known
        assert_eq!(match_by_content(&own, "A.RAR", "", &donor), None);
    }

    #[test]
    fn nzb_reading() {
        let xml = br#"<?xml version="1.0"?><nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">
<file poster="p" date="1" subject="[1/2] - &quot;rel/x.par2&quot; yEnc (1/1)"><groups><group>a.b</group></groups>
<segments><segment bytes="100" number="1">id1@x</segment></segments></file>
<file subject="&quot;movie.mkv&quot; yEnc (1/2)"><groups><group>a.b</group><group>c.d</group></groups>
<segments><segment bytes="5" number="1">a b
c@x</segment><segment bytes="7" number="2">id3@x</segment></segments></file>
<file subject="empty"><segments></segments></file></nzb>"#;
        let entries = read_nzb(xml);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].filename, "rel_x.par2");
        assert!(entries[0].is_par2());
        assert_eq!(entries[1].groups, vec!["a.b", "c.d"]);
        assert_eq!(
            entries[1].segments,
            vec![(5, "<a_b_c@x>".into()), (7, "<id3@x>".into())]
        );
        assert_eq!(entries[1].size(), 12);
    }

    #[test]
    fn par2_of_a_real_set() {
        // made by par2cmdline when it's there: rust-par2 reads the FileDesc packets
        let dir = std::env::temp_dir().join(format!("twin-par2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let data: Vec<u8> = (0..300_000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();
        std::fs::write(dir.join("movie.mkv"), &data).unwrap();
        let made = std::process::Command::new("par2")
            .args([
                "create",
                "-q",
                "-q",
                "-s65536",
                "-c2",
                "m.par2",
                "movie.mkv",
            ])
            .current_dir(&dir)
            .status();
        if !matches!(made, Ok(s) if s.success()) {
            return; // no par2cmdline here
        }
        let par2 = std::fs::read(dir.join("m.par2")).unwrap();
        let (sigs, complete) = par2_set(&par2);
        assert!(complete);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "movie.mkv");
        assert_eq!(sigs[0].length, 300_000);
        let md5 = std::process::Command::new("md5sum")
            .arg(dir.join("movie.mkv"))
            .output()
            .unwrap();
        assert_eq!(sigs[0].md5, String::from_utf8_lossy(&md5.stdout)[..32]);
        // a lost article took the Main packet's header (it comes first): the
        // FileDesc packet after it still tells the file
        let mut damaged = par2.clone();
        for b in damaged.iter_mut().take(64) {
            *b = 0;
        }
        let (sigs, complete) = par2_set(&damaged);
        assert_eq!(sigs.len(), 1);
        assert!(
            !complete,
            "without its Main packet a list can't be known complete"
        );
        // zeros through the FileDesc packet too (a lost article): not taken
        let desc = par2
            .windows(16)
            .position(|w| w == b"PAR 2.0\0FileDesc")
            .unwrap()
            - 48;
        let mut torn = damaged.clone();
        for b in torn.iter_mut().skip(desc + 70).take(16) {
            *b = 0;
        }
        assert!(
            par2_set(&torn).0.is_empty(),
            "a FileDesc packet with zeros in it is rejected"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nzb_in_a_declared_legacy_encoding() {
        // ISO-8859-1, "r\xe9lease.par2": still read, still a par2-file
        let mut xml =
            b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><nzb><file subject=\"&quot;r".to_vec();
        xml.push(0xe9);
        xml.extend_from_slice(b"lease.par2&quot; yEnc (1/1)\"><groups><group>a.b</group></groups><segments><segment bytes=\"9\" number=\"1\">id@x</segment></segments></file></nzb>");
        let entries = read_nzb(&xml);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "r\u{e9}lease.par2");
        assert!(entries[0].is_par2());
    }

    #[test]
    fn store_round_trip() {
        let path = std::env::temp_dir().join(format!("twincheck-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut store = Store::open(path.clone());
        store.put(7, listed(vec![sig("a\tb", "1", 1)], true));
        let second = listed(vec![sig("c", "2", 2)], false);
        store.put(7, second.clone());
        let third = listed(vec![sig("d", "3", 3)], true);
        store.put(8, third.clone());
        // a record cut short (no end line): ignored, the earlier one stays
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"8\tnew\tother\t1\n8\t9\t9\tz\n")
            .unwrap();
        let again = Store::open(path.clone());
        assert_eq!(again.get(7, &second.print), &second);
        assert_eq!(again.get(8, &third.print), &third);
        // another posting under a reused id: its list isn't this one
        assert!(again.get(8, "other").sigs.is_empty());
        assert!(again.get(9, &third.print).sigs.is_empty());
        // pruned to the postings still there: the file keeps them alone
        let mut again = again;
        again.prune(&[(8, third.print.clone()), (7, "replaced".into())]);
        let pruned = Store::open(path.clone());
        assert_eq!(pruned.get(8, &third.print), &third);
        assert!(pruned.get(7, &second.print).sigs.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    /// fingerprint as it was before: a BTreeSet of "md5:length" strings.
    fn fingerprint_before(sigs: &[FileSig]) -> String {
        let files: BTreeSet<String> = sigs
            .iter()
            .map(|s| format!("{}:{}", s.md5, s.length))
            .collect();
        if files.is_empty() {
            return String::new();
        }
        let mut hash: u64 = 14695981039346656037;
        for file in &files {
            for byte in file.bytes().chain(std::iter::once(b'\n')) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(1099511628211);
            }
        }
        format!("{hash:016x}-{}", files.len())
    }

    #[test]
    fn fingerprint_is_the_one_stored_before() {
        // values the C++ stored for these lists (and the first version of this port)
        let sig = |md5: &str, length| FileSig {
            name: String::new(),
            length,
            md5: md5.to_string(),
        };
        assert_eq!(fingerprint(&[]), "");
        let mut rng = Rng(0xD1B54A32D192ED03);
        for round in 0..20000 {
            let n = rng.below(40) as usize;
            let sigs: Vec<FileSig> = (0..n)
                .map(|_| {
                    // md5 texts of 32 hex digits, short and long ones, repeated and prefix-sharing, lengths of 1 to 20 digits
                    let width = [32, 32, 32, 1, 2, 3, 31, 33, 40, 60, 70][rng.below(11) as usize];
                    let md5: String = (0..width)
                        .map(|_| {
                            b"0123456789abcdef:"
                                [rng.below(if width > 3 { 16 } else { 17 }) as usize]
                                as char
                        })
                        .collect();
                    let md5 = if rng.below(4) == 0 {
                        md5[..md5.len().min(2)].to_string()
                    } else {
                        md5
                    };
                    let length = [
                        0u64,
                        9,
                        10,
                        99,
                        100,
                        1000,
                        rng.next(),
                        rng.next() >> 40,
                        u64::MAX,
                    ][rng.below(9) as usize];
                    sig(&md5, length)
                })
                .collect();
            let mut doubled = sigs.clone();
            if rng.below(3) == 0 {
                doubled.extend(sigs.iter().take(5).cloned());
            }
            assert_eq!(
                fingerprint(&doubled),
                fingerprint_before(&doubled),
                "round {round}"
            );
        }
        // sorted as text: "9" after "10" under one md5
        let same = [sig("aa", 9), sig("aa", 10), sig("aa", 100)];
        assert_eq!(fingerprint(&same), fingerprint_before(&same));
    }

    #[test]
    fn main_non_recoverable_files_require_descriptions() {
        let mut rng = Rng(1);
        let mut data = main_packet(65536, &[1, 2], 1);
        data.extend(file_desc(1, "movie.mkv", 1024, &mut rng));
        let (sigs, complete) = par2_set(&data);
        assert_eq!(sigs.len(), 1);
        assert!(!complete, "the non-recoverable file description is missing");
        assert_eq!(
            walk(&data, true).0.unwrap().file_order,
            vec![[1; 16], [2; 16]]
        );
        data.extend(file_desc(2, "release.nfo", 128, &mut rng));
        let (sigs, complete) = par2_set(&data);
        assert_eq!(sigs.len(), 2);
        assert!(complete);

        let mut data = main_packet(65536, &[2], 0);
        data.extend(file_desc(2, "release.nfo", 128, &mut rng));
        assert!(
            par2_set(&data).1,
            "a set with only non-recoverable files is complete"
        );
    }

    #[test]
    fn file_id_hashing_uses_every_byte_and_randomized_state() {
        use std::hash::BuildHasher;
        let map: IdMap<()> = IdMap::default();
        let hashes: std::collections::HashSet<_> = (0..256u32)
            .map(|i| {
                let mut id = [7u8; 16];
                id[8..12].copy_from_slice(&i.to_le_bytes());
                map.hasher().hash_one(id)
            })
            .collect();
        assert_eq!(hashes.len(), 256, "distinct IDs must use their second half");
        let hashes: std::collections::HashSet<_> = (0..8)
            .map(|_| IdMap::<()>::default().hasher().hash_one([7u8; 16]))
            .collect();
        assert!(
            hashes.len() > 1,
            "independent maps must use randomized hash state"
        );
    }

    #[test]
    fn malformed_main_recoverable_count_leaves_list_incomplete() {
        let mut rng = Rng(72);
        for count in [2, u32::MAX] {
            let mut data = main_packet(4096, &[1], count);
            data.extend_from_slice(&file_desc(1, "a.bin", 10, &mut rng));
            let (sigs, complete) = par2_set(&data);
            assert_eq!(sigs.len(), 1);
            assert_eq!(sigs[0].name, "a.bin");
            assert_eq!(sigs[0].length, 10);
            assert!(
                !complete,
                "more recoverable files declared than IDs present"
            );
        }
    }

    #[test]
    fn malformed_main_partial_file_id_leaves_list_incomplete() {
        for tail in [4, 8, 12] {
            let mut body = 4096u64.to_le_bytes().to_vec();
            body.extend_from_slice(&1u32.to_le_bytes());
            body.extend_from_slice(&[1; 16]);
            body.extend(std::iter::repeat_n(0, tail));
            let mut data = packet([7; 16], TYPE_MAIN, &body);
            data.extend_from_slice(&file_desc(1, "a.bin", 10, &mut Rng(73)));
            let (sigs, complete) = par2_set(&data);
            assert_eq!(sigs.len(), 1);
            assert_eq!(sigs[0].name, "a.bin");
            assert_eq!(sigs[0].length, 10);
            assert!(!complete, "a partial file ID cannot prove a complete list");
        }
    }

    // ---- the walk against rust-par2's parser ----

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn packet(set_id: [u8; 16], kind: &[u8; 16], body: &[u8]) -> Vec<u8> {
        use md5::{Digest, Md5};
        let mut data = set_id.to_vec();
        data.extend_from_slice(kind);
        data.extend_from_slice(body);
        let mut out = b"PAR2\0PKT".to_vec();
        out.extend_from_slice(&((data.len() + 32) as u64).to_le_bytes());
        out.extend_from_slice(&Md5::digest(&data));
        out.extend_from_slice(&data);
        out
    }

    fn file_desc(id: u8, name: &str, size: u64, rng: &mut Rng) -> Vec<u8> {
        let mut body = vec![id; 16];
        body.extend((0..32).map(|_| rng.next() as u8));
        body.extend_from_slice(&size.to_le_bytes());
        body.extend_from_slice(name.as_bytes());
        body.resize(body.len().div_ceil(4) * 4, 0);
        packet([7; 16], TYPE_FILE_DESC, &body)
    }

    fn main_packet(slice: u64, ids: &[u8], count: u32) -> Vec<u8> {
        let mut body = slice.to_le_bytes().to_vec();
        body.extend_from_slice(&count.to_le_bytes());
        for &id in ids {
            body.extend_from_slice(&[id; 16]);
        }
        packet([7; 16], TYPE_MAIN, &body)
    }

    fn random_set(rng: &mut Rng, valid_main_counts: bool) -> Vec<u8> {
        let files = 1 + rng.below(6) as u8;
        let ids: Vec<u8> = (1..=files).collect();
        let mut packets: Vec<Vec<u8>> = Vec::new();
        let slice = 1 << (10 + rng.below(8));
        let extra = rng.below(2) as u32;
        packets.push(main_packet(
            slice,
            &ids,
            files as u32 + if valid_main_counts { 0 } else { extra },
        ));
        for &id in &ids {
            let name = ["a.mkv", "b.rar", "caf\u{e9}.nfo", "x y.part01.rar", "c.bin"]
                [rng.below(5) as usize];
            packets.push(file_desc(id, name, rng.next() >> 20, rng));
            let mut body = vec![id; 16];
            body.extend((0..20 * rng.below(5)).map(|_| rng.next() as u8));
            packets.push(packet([7; 16], b"PAR 2.0\0IFSC\0\0\0\0", &body));
        }
        for _ in 0..rng.below(4) {
            let mut body = vec![0u8; 4];
            body.extend((0..4 * (1 + rng.below(300))).map(|_| rng.next() as u8));
            packets.push(packet([7; 16], b"PAR 2.0\0RecvSlic", &body));
        }
        packets.push(packet([7; 16], b"PAR 2.0\0Creator\0", b"test\0\0\0\0"));
        // mutations
        for _ in 0..rng.below(4) {
            let n = rng.below(packets.len() as u64) as usize;
            let kind = rng.below(9);
            if matches!(kind, 0 | 1 | 6 | 7) && packets[n].len() < 64 {
                continue; // junk inserted earlier: nothing to damage
            }
            match kind {
                0 => {
                    let at = 16 + rng.below(16) as usize;
                    packets[n][at] ^= 1 + rng.below(255) as u8; // header MD5 wrong
                }
                1 => {
                    let at = 32 + rng.below(packets[n].len() as u64 - 32) as usize;
                    packets[n][at] ^= 1 + rng.below(255) as u8; // body damaged
                }
                2 => packets.insert(n, vec![0x55; rng.below(30) as usize]), // junk between
                3 => packets.insert(n, b"PAR2\0PKT".to_vec()), // a magic with nothing after
                4 => {
                    let dup = packets[n].clone();
                    packets.insert(n, dup); // a duplicate packet
                }
                5 => {
                    // the same packet again, other values: a second Main or FileDesc
                    packets.push(if rng.below(2) == 0 {
                        let slice = 1 << (10 + rng.below(8));
                        let n = rng.below(files as u64) as usize + 1;
                        main_packet(
                            slice,
                            &ids[..n],
                            if valid_main_counts {
                                n as u32
                            } else {
                                files as u32
                            },
                        )
                    } else {
                        file_desc(ids[rng.below(files as u64) as usize], "dup.bin", 1, rng)
                    });
                }
                6 => {
                    let len =
                        [0u64, 8, 63, 65, 66, 100, 4096, 1 << 30, 1 << 40][rng.below(9) as usize];
                    packets[n][8..16].copy_from_slice(&len.to_le_bytes()); // a wrong length
                }
                7 => {
                    // a huge declared length with the header's bytes after it
                    let mut p = packets[n].clone();
                    p[8..16].copy_from_slice(
                        &(10 * 1024 * 1024 + 64 + 4 * rng.below(100)).to_le_bytes(),
                    );
                    p.truncate(16 + rng.below(80) as usize);
                    packets.insert(n, p);
                }
                _ => {
                    packets.remove(n);
                }
            }
        }
        let mut data: Vec<u8> = packets.concat();
        if rng.below(3) == 0 && !data.is_empty() {
            data.truncate(rng.below(data.len() as u64 + 1) as usize); // cut short anywhere
        }
        if rng.below(4) == 0 {
            data.splice(0..0, (0..rng.below(20)).map(|_| 0x11u8)); // junk first
        }
        data
    }

    #[test]
    fn par2_set_and_block_size_handle_corrupted_sets_and_match_par_rs_twin() {
        let mut rng = Rng(0xA0761D6478BD642F);
        let mut sizes = 0;
        for round in 0..5000 {
            let mut data = random_set(&mut rng, true);
            if rng.below(6) == 0 {
                let junk = [4usize, 1, 2, 3, 5, 6, 7, 8, 12][rng.below(9) as usize];
                data.splice(0..0, std::iter::repeat_n(0x22u8, junk));
            }
            if rng.below(8) == 0 {
                data.splice(0..0, std::iter::repeat_n(0u8, 4 * rng.below(40) as usize));
            }
            let (sigs, complete) = par2_set(&data);
            if complete {
                assert!(
                    !sigs.is_empty(),
                    "complete set must have files, round {round}"
                );
            }
            let size = block_size(&data);
            if size > 0 {
                sizes += 1;
            }
        }
        assert!(sizes > 1000, "{sizes} sets with a block size");
    }

    #[test]
    fn walk_ends_at_a_length_past_the_data() {
        // a length that wraps negative ended in a loop in the crate
        let mut data = main_packet(4096, &[1], 1);
        let mut bad = b"PAR2\0PKT".to_vec();
        bad.extend_from_slice(&0xFFFF_FFFF_FFFF_FFC0u64.to_le_bytes());
        bad.extend_from_slice(&[0u8; 64]);
        data.extend_from_slice(&bad);
        data.extend_from_slice(&file_desc(1, "a", 1, &mut Rng(1)));
        let set = walk(&data, true).0.unwrap();
        assert_eq!(set.slice_size, 4096);
        assert!(set.files.is_empty());
    }
}

#[cfg(test)]
mod timing {
    use super::*;
    use std::time::Instant;

    fn best(f: impl Fn()) -> f64 {
        let mut best = f64::MAX;
        for _ in 0..21 {
            let t = Instant::now();
            for _ in 0..200 {
                f();
            }
            best = best.min(t.elapsed().as_secs_f64() / 200.0);
        }
        best * 1e9
    }

    #[test]
    #[ignore]
    fn par2_timing() {
        let dir = "/tmp/claude-1000/perf-a/";
        for name in ["par2_small.bin", "par2_idx.bin", "par2_vol.bin"] {
            let data = std::fs::read(format!("{dir}{name}")).unwrap();
            let walk_files = best(|| drop(std::hint::black_box(walk(&data, true).0)));
            let walk_main = best(|| drop(std::hint::black_box(walk(&data, false).0)));
            let set = best(|| drop(std::hint::black_box(par2_set(&data))));
            let sigs = par2_set(&data).0;
            let text = best(|| {
                let t: String = sigs
                    .iter()
                    .map(|s| format!("{}\t{}\t{}\n", s.length, s.md5, s.name))
                    .collect();
                std::hint::black_box(t);
            });
            let md5_only = best(|| {
                use md5::{Digest, Md5};
                let mut pos = 0;
                let mut n = 0;
                while pos + 16 <= data.len() {
                    let len =
                        u64::from_le_bytes(data[pos + 8..pos + 16].try_into().unwrap()) as usize;
                    if len < 64 || pos + len > data.len() {
                        break;
                    }
                    if &data[pos + 48..pos + 64] == TYPE_FILE_DESC {
                        std::hint::black_box(Md5::digest(&data[pos + 32..pos + len]));
                        n += 1;
                    }
                    pos += len;
                }
                std::hint::black_box(n);
            });
            let sigs0 = par2_set(&data).0;
            if sigs0.len() == 30 {
                let fp = best(|| drop(std::hint::black_box(fingerprint(&sigs0))));
                let text: String = sigs0
                    .iter()
                    .map(|s| format!("{}\t{}\n", s.length, s.md5))
                    .collect();
                let parse = best(|| {
                    let files = text.lines().filter_map(|line| {
                        let mut parts = line.splitn(3, '\t');
                        let length: u64 = parts.next()?.parse().ok()?;
                        Some((parts.next()?, length))
                    });
                    std::hint::black_box(files.count());
                });
                eprintln!("fingerprint of 30: {fp:.0} ns, parsing its lines: {parse:.0} ns");
            }
            eprintln!("md5 of the FileDesc packets: {md5_only:.0} ns");
            eprintln!("{name}: walk(files) {walk_files:.0} ns, walk(main) {walk_main:.0} ns, par2_set {set:.0} ns, text {text:.0} ns");
        }
    }
}
