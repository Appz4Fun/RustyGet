//! Donor articles fetched to map or sample a donor copy (a twin or alt), kept on disk by message-id so no
//! later pass fetches them again.
//!
//! Mapping an alt walks every volume of its container (`ContentMapper::build_map_with_tls`):
//! the first article of each volume, and the last one where the end-of-archive block lies.
//! TwinCheck sampling and the fleet intake read the same header and tail articles. Every
//! repair run of a download builds its donor's map again (one run per hole), and the borrow
//! starts with the header article of the volume it reads. The in-memory memo of recent
//! articles (`streamrepair_coord::RECENT_SINGLES`, 64 articles / 48 MB) holds less than one
//! walk of a large set (AltB: 85 volumes, 170 articles, 88 MB), so it evicted everything
//! before the next run asked again: on the 4 GiB alt failover run each of two runs fetched
//! all 170 articles (finding 6 of the 2026-11-26 audit).
//!
//! This store keeps those articles in files under `<TempDir>/rustyget-donor-articles-<pid>/`
//! and only their index in memory. A message-id names the same article bytes on every
//! server, so an article is served from here for any later run, check or borrow. Each entry
//! is owned by the downloads it was fetched for; [`forget`] drops a download's entries when
//! it leaves the queue (or starts over), and the whole store is capped at
//! [`STORE_MAX_BYTES`] and [`STORE_MAX_ARTICLES`] (oldest out first).
//!
//! Only articles that came back whole are kept; a failed or missing article is never stored.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Most bytes the store keeps on disk.
pub const STORE_MAX_BYTES: u64 = 512 << 20;
/// Most articles the store keeps.
pub const STORE_MAX_ARTICLES: usize = 4096;
/// An article bigger than this is not kept (news articles are well under 1 MB).
pub const STORE_MAX_ARTICLE: usize = 8 << 20;

/// Owner of an entry fetched by a check that belongs to no download in particular: it goes
/// only when the caps push it out.
pub const NO_OWNER: i32 = -1;

const DIR_PREFIX: &str = "rustyget-donor-articles-";

/// A stored article as it was fetched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredArticle {
    pub offset: i64,
    pub file_size: i64,
    pub data: Vec<u8>,
}

struct Entry {
    file: PathBuf,
    len: u64,
    offset: i64,
    file_size: i64,
    owners: BTreeSet<i32>,
}

#[derive(Default)]
struct Store {
    dir: Option<PathBuf>,
    next: u64,
    bytes: u64,
    entries: HashMap<Vec<u8>, Entry>,
    order: VecDeque<Vec<u8>>,
    /// Articles served from the store, and articles put into it (counters for logs and tests).
    hits: u64,
    puts: u64,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<Store>> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(unix)]
fn path_of(b: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(b))
}

#[cfg(not(unix))]
fn path_of(b: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(b).into_owned())
}

fn base_dir() -> PathBuf {
    if let Some(opts) = crate::options::active_options() {
        let tmp = opts.str_bytes(crate::options::OptStr::TempDir);
        if !tmp.is_empty() {
            return path_of(tmp);
        }
    }
    std::env::temp_dir()
}

/// Removes the stores that earlier processes left behind (a crash, a kill): a directory of
/// this name whose process no longer runs. Another daemon sharing the temp folder keeps its own.
fn remove_stale(base: &Path) {
    let Ok(rd) = std::fs::read_dir(base) else {
        return;
    };
    for de in rd.flatten() {
        let name = de.file_name();
        // `<prefix><pid>-<nonce>`
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix(DIR_PREFIX))
            .and_then(|p| p.split('-').next())
            .and_then(|p| p.parse::<i32>().ok())
        else {
            continue;
        };
        // One of this process id is from an earlier run (a container's daemon gets the same
        // small id each start): this process makes its own only after this cleanup.
        let own = pid as u32 == std::process::id();
        if pid <= 0 || (!own && process_alive(pid)) {
            continue;
        }
        let path = de.path();
        // a real directory only, never one reached through a symlink
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

#[cfg(unix)]
fn process_alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    let r = unsafe { libc::kill(pid, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(not(unix))]
fn process_alive(_pid: i32) -> bool {
    true
}

/// A new directory only this user can enter, named for this process and a nonce: never one
/// that exists already (another daemon's, a reused process id's, or one planted in a shared
/// temp folder), and never through a symlink.
fn create_private_dir(base: &Path) -> Option<PathBuf> {
    use std::hash::{BuildHasher, Hasher};
    for _ in 0..8 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        let dir = base.join(format!("{DIR_PREFIX}{}-{:016x}", std::process::id(), h.finish()));
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        match b.create(&dir) {
            Ok(()) => return Some(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

fn store_dir(s: &mut Store) -> Option<PathBuf> {
    if let Some(d) = &s.dir {
        return Some(d.clone());
    }
    let dir = create_private_dir(&base_dir())?;
    s.dir = Some(dir.clone());
    Some(dir)
}

/// Stores of earlier processes are removed once, before the first article is kept, outside
/// the store's lock (a large one takes a while).
static CLEANED: std::sync::Once = std::sync::Once::new();

fn drop_entry(s: &mut Store, msg_id: &[u8]) {
    if let Some(e) = s.entries.remove(msg_id) {
        s.bytes = s.bytes.saturating_sub(e.len);
        let _ = std::fs::remove_file(&e.file);
    }
}

/// The message-id without its angle brackets: callers name an article either way.
fn key(msg_id: &[u8]) -> &[u8] {
    let m = msg_id.strip_prefix(b"<").unwrap_or(msg_id);
    m.strip_suffix(b">").unwrap_or(m)
}

/// The bytes of `path` as [`put`] wrote them for `msg_id` (the key's length in 4 bytes, the
/// key, then `len` article bytes), or `None` unless it is a regular file of exactly that size
/// holding that key: a file of another article is never served under this message-id.
fn read_bounded(path: &Path, msg_id: &[u8], len: u64) -> Option<Vec<u8>> {
    let head = 4 + msg_id.len() as u64;
    let mut f = std::fs::File::open(path).ok()?;
    let meta = f.metadata().ok()?;
    if !meta.is_file() || meta.len() != head + len {
        return None;
    }
    let mut key_len = [0u8; 4];
    f.read_exact(&mut key_len).ok()?;
    let mut stored_key = vec![0u8; usize::try_from(u32::from_le_bytes(key_len)).ok()?];
    if stored_key.len() != msg_id.len() {
        return None;
    }
    f.read_exact(&mut stored_key).ok()?;
    if stored_key != msg_id {
        return None;
    }
    let mut data = vec![0u8; usize::try_from(len).ok()?];
    f.read_exact(&mut data).ok()?;
    Some(data)
}

/// The article stored under `msg_id`, now also owned by `owner`; `None` when none is held or
/// its file can no longer be read whole (the entry is dropped then).
pub fn get(owner: i32, msg_id: &[u8]) -> Option<StoredArticle> {
    let msg_id = key(msg_id);
    let (file, len, offset, file_size) = {
        let mut g = lock();
        let s = g.as_mut()?;
        let e = s.entries.get_mut(msg_id)?;
        if owner != NO_OWNER {
            e.owners.insert(owner);
        }
        (e.file.clone(), e.len, e.offset, e.file_size)
    };
    match read_bounded(&file, msg_id, len) {
        Some(data) => {
            if let Some(s) = lock().as_mut() {
                s.hits += 1;
            }
            Some(StoredArticle {
                offset,
                file_size,
                data,
            })
        }
        None => {
            if let Some(s) = lock().as_mut() {
                // only the entry that pointed at this file
                if s.entries.get(msg_id).is_some_and(|e| e.file == file) {
                    drop_entry(s, msg_id);
                    s.order.retain(|id| id.as_slice() != msg_id);
                }
            }
            None
        }
    }
}

/// True when an article is stored under `msg_id` (no file read).
pub fn contains(msg_id: &[u8]) -> bool {
    let msg_id = key(msg_id);
    lock()
        .as_ref()
        .is_some_and(|s| s.entries.contains_key(msg_id))
}

/// Keeps an article fetched whole for `owner`. One already held only gains the owner.
pub fn put(owner: i32, msg_id: &[u8], offset: i64, file_size: i64, data: &[u8]) {
    let msg_id = key(msg_id);
    if msg_id.is_empty() || msg_id.len() > 1024 || data.is_empty() || data.len() > STORE_MAX_ARTICLE {
        return;
    }
    CLEANED.call_once(|| remove_stale(&base_dir()));
    let file = {
        let mut g = lock();
        let s = g.get_or_insert_with(Store::default);
        if let Some(e) = s.entries.get_mut(msg_id) {
            if owner != NO_OWNER {
                e.owners.insert(owner);
            }
            return;
        }
        let Some(dir) = store_dir(s) else {
            return;
        };
        s.next += 1;
        dir.join(format!("{}.art", s.next))
    };
    // written outside the lock; the entry appears only once the file is whole
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
        .and_then(|mut f| {
            let key_len = u32::try_from(msg_id.len()).unwrap_or(u32::MAX);
            f.write_all(&key_len.to_le_bytes())?;
            f.write_all(msg_id)?;
            f.write_all(data)
        });
    if written.is_err() {
        let _ = std::fs::remove_file(&file);
        return;
    }
    let len = data.len() as u64;
    let mut g = lock();
    let Some(s) = g.as_mut() else {
        let _ = std::fs::remove_file(&file);
        return;
    };
    if let Some(e) = s.entries.get_mut(msg_id) {
        // another thread stored it meanwhile
        if owner != NO_OWNER {
            e.owners.insert(owner);
        }
        let _ = std::fs::remove_file(&file);
        return;
    }
    while !s.order.is_empty()
        && (s.entries.len() >= STORE_MAX_ARTICLES || s.bytes + len > STORE_MAX_BYTES)
    {
        if let Some(old) = s.order.pop_front() {
            drop_entry(s, &old);
        }
    }
    let mut owners = BTreeSet::new();
    if owner != NO_OWNER {
        owners.insert(owner);
    }
    s.entries.insert(
        msg_id.to_vec(),
        Entry {
            file,
            len,
            offset,
            file_size,
            owners,
        },
    );
    s.order.push_back(msg_id.to_vec());
    s.bytes += len;
    s.puts += 1;
}

/// Drops `owner` from every entry; entries no other download owns go (the download left the
/// queue or starts over). Entries of [`NO_OWNER`] stay until the caps push them out.
pub fn forget(owner: i32) {
    if owner == NO_OWNER {
        return;
    }
    let mut g = lock();
    let Some(s) = g.as_mut() else {
        return;
    };
    let mut gone = Vec::new();
    for (id, e) in &mut s.entries {
        if e.owners.remove(&owner) && e.owners.is_empty() {
            gone.push(id.clone());
        }
    }
    for id in &gone {
        drop_entry(s, id);
    }
    s.order.retain(|id| s.entries.contains_key(id));
}

/// (articles held, bytes held, articles served, articles stored) since the process started.
pub fn stats() -> (usize, u64, u64, u64) {
    lock()
        .as_ref()
        .map_or((0, 0, 0, 0), |s| (s.entries.len(), s.bytes, s.hits, s.puts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_returns_and_forgets_by_owner() {
        let id = format!("<donorstore-test-{}@x>", std::process::id()).into_bytes();
        let id2 = format!("<donorstore-test2-{}@x>", std::process::id()).into_bytes();
        assert!(get(7001, &id).is_none());
        put(7001, &id, 716_800, 51_200_000, b"header bytes");
        put(7002, &id2, 0, 51_200_000, b"tail bytes");
        assert!(contains(&id));
        let got = get(7003, &id).expect("stored");
        assert_eq!(got.offset, 716_800);
        assert_eq!(got.file_size, 51_200_000);
        assert_eq!(got.data, b"header bytes");
        // still owned by 7003 after 7001 forgets it
        forget(7001);
        assert!(contains(&id));
        forget(7003);
        assert!(!contains(&id));
        assert!(contains(&id2));
        forget(7002);
        assert!(!contains(&id2));
    }

    #[test]
    fn a_stored_file_that_changed_size_is_not_served() {
        let id = format!("<donorstore-trunc-{}@x>", std::process::id()).into_bytes();
        put(7010, &id, 0, 10, b"0123456789");
        let file = lock()
            .as_ref()
            .and_then(|s| s.entries.get(key(&id)).map(|e| e.file.clone()))
            .expect("entry");
        std::fs::write(&file, b"0123").unwrap();
        assert!(get(7010, &id).is_none());
        assert!(!contains(&id));
    }

    #[test]
    fn a_file_of_another_article_is_never_served() {
        let id = format!("<donorstore-key-{}@x>", std::process::id()).into_bytes();
        put(7040, &id, 0, 4, b"abcd");
        let file = lock()
            .as_ref()
            .and_then(|s| s.entries.get(key(&id)).map(|e| e.file.clone()))
            .expect("entry");
        // the same size, the bytes of an article under another message-id of equal length
        let other = key(&id).iter().map(|&b| if b == b'k' { b'K' } else { b }).collect::<Vec<u8>>();
        let mut forged = (other.len() as u32).to_le_bytes().to_vec();
        forged.extend_from_slice(&other);
        forged.extend_from_slice(b"wxyz");
        std::fs::write(&file, &forged).unwrap();
        assert!(get(7040, &id).is_none());
        assert!(!contains(&id));
    }

    #[test]
    fn a_message_id_with_or_without_brackets_is_one_article() {
        let bare = format!("donorstore-br-{}@x", std::process::id());
        put(7030, format!("<{bare}>").as_bytes(), 0, 3, b"abc");
        assert_eq!(get(7030, bare.as_bytes()).map(|a| a.data), Some(b"abc".to_vec()));
        forget(7030);
        assert!(!contains(bare.as_bytes()));
    }

    #[test]
    fn empty_and_oversized_articles_are_not_kept() {
        let id = format!("<donorstore-empty-{}@x>", std::process::id()).into_bytes();
        put(7020, &id, 0, 0, b"");
        assert!(!contains(&id));
        put(7020, &id, 0, 0, &vec![0u8; STORE_MAX_ARTICLE + 1]);
        assert!(!contains(&id));
    }
}
