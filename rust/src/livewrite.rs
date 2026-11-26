//! Writer-fed live extraction: the volumes a download is still writing, article by article.
//!
//! A live extraction used to wait for each volume to be finished and renamed, then read it
//! back; the last volume (often hundreds of MB) was only decoded after the download ended. The
//! article writer now reports every article it has written into a volume's direct-write output
//! (`written`, after its write returned), and the extraction reads those bytes right away
//! through zipfast's `map::VolumeFiles` (`set_file`, `mark_written`; see [`LiveVolume::sync`]):
//! when the last article lands, nearly everything before it is already extracted.
//!
//! Only direct-write outputs are reported: a file assembled from article temporaries (direct
//! write off, dupe-fallback articles) has no bytes in place until it completes, and is read
//! when it is finished as before. Lost articles are never reported here: a volume that
//! completes with holes is handed over with them by the download's feed
//! (`progressive_unpack::VolumeFeed`), exactly as a finished volume is.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::Waker;
use std::time::Duration;

/// A volume being written by the download.
#[derive(Debug)]
pub struct LiveVolume {
    nzb_id: i32,
    file_id: i32,
    /// The file the writer writes the articles into (its direct-write output).
    out_path: PathBuf,
    /// The volume's decoded length (its yEnc `size`), which the output was created with.
    size: u64,
    state: Mutex<State>,
    read_live: AtomicU64,
}

#[derive(Debug, Default)]
struct State {
    /// The full path the volume will have once complete, as the download names it now.
    final_path: Vec<u8>,
    /// Byte ranges written, sorted and disjoint.
    written: Vec<Range<u64>>,
    /// A reader waiting for an offset not written yet, woken (when parked) once it is. The
    /// article cache writes the volume's articles out while someone waits (see [`wanted`]).
    waiting: Option<(u64, Option<Waker>)>,
    /// The download dropped this volume (finished, moved or deleted): nothing more comes.
    forgotten: bool,
    /// Bumped by every write, so a reader hands over only what changed (see `sync`).
    generation: u64,
    /// Its first bytes (offset 0, the headers) are written.
    head: bool,
}

impl LiveVolume {
    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn out_path(&self) -> &Path {
        &self.out_path
    }

    /// The written run holding `offset`: its end, or `None` when `offset` is not written yet.
    pub fn written_end(&self, offset: u64) -> Option<u64> {
        let st = lock(&self.state);
        run_holding(&st.written, offset).map(|r| r.end)
    }

    /// Where the next written run after `offset` starts, if any.
    pub fn next_written(&self, offset: u64) -> Option<u64> {
        let st = lock(&self.state);
        let at = st.written.partition_point(|r| r.end <= offset);
        st.written.get(at).map(|r| r.start.max(offset))
    }

    /// Parks `waker` until `offset` is written (or the volume is dropped). False when it is
    /// written already, or nothing more comes: the caller reads again instead of waiting.
    pub fn wait_for(&self, offset: u64, waker: &Waker) -> bool {
        let mut st = lock(&self.state);
        if st.forgotten || run_holding(&st.written, offset).is_some() {
            return false;
        }
        st.waiting = Some((offset, Some(waker.clone())));
        true
    }

    /// A reader that polls (no waker) waits for `offset`: the article cache writes this
    /// volume's articles out instead of holding them (see [`wanted`]).
    pub fn want(&self, offset: u64) {
        let mut st = lock(&self.state);
        if !st.forgotten && run_holding(&st.written, offset).is_none() {
            if !matches!(st.waiting, Some((_, Some(_)))) {
                st.waiting = Some((offset, None));
            }
        }
    }

    /// Gives zipfast's writer-fed source the written ranges when anything was written since
    /// the last call (`synced` is the write generation handed over; marking a range again is
    /// harmless).
    pub fn sync(&self, files: &mut zipfast::map::VolumeFiles, volume: u32, synced: &mut u64) {
        let st = lock(&self.state);
        if st.generation == *synced {
            return;
        }
        for r in &st.written {
            files.mark_written(volume, r.clone());
        }
        *synced = st.generation;
    }

    /// The reader no longer waits for this volume's writes (it reads the finished volume, gave
    /// up, or is gone): the article cache stops writing this file out early for it.
    pub fn stop_waiting(&self) {
        lock(&self.state).waiting = None;
    }

    /// Whether a reader waits for a write into this volume.
    pub fn is_waited(&self) -> bool {
        lock(&self.state).waiting.is_some()
    }

    /// Packed bytes extractions read from this volume as it was written.
    pub fn read_live(&self) -> u64 {
        self.read_live.load(Ordering::Relaxed)
    }

    /// Counts `n` bytes an extraction read from this volume as it was written.
    pub fn count_live(&self, n: u64) {
        self.read_live.fetch_add(n, Ordering::Relaxed);
        count_read(n, ReadKind::Live);
    }

    /// Records a write; also says whether it was the first one holding offset 0 of a first
    /// volume (see [`take_new_heads`]).
    fn written(&self, range: Range<u64>) -> (Option<Waker>, bool) {
        let mut st = lock(&self.state);
        insert(&mut st.written, range);
        st.generation += 1;
        let new_head = !st.head && run_holding(&st.written, 0).is_some();
        st.head |= new_head;
        let first_volume = new_head && is_first_volume(&st.final_path);
        let waker = match st.waiting.take() {
            Some((offset, waker)) if run_holding(&st.written, offset).is_some() => waker,
            other => {
                st.waiting = other;
                None
            }
        };
        (waker, first_volume)
    }

    fn forget(&self) -> Option<Waker> {
        let mut st = lock(&self.state);
        st.forgotten = true;
        st.waiting.take().and_then(|(_, w)| w)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn run_holding(runs: &[Range<u64>], offset: u64) -> Option<&Range<u64>> {
    let at = runs.partition_point(|r| r.end <= offset);
    runs.get(at).filter(|r| r.start <= offset)
}

/// Adds `range` to sorted, disjoint `runs`, merging what touches it.
fn insert(runs: &mut Vec<Range<u64>>, range: Range<u64>) {
    if range.start >= range.end {
        return;
    }
    let first = runs.partition_point(|r| r.end < range.start);
    let last = runs.partition_point(|r| r.start <= range.end);
    if first == last {
        runs.insert(first, range);
        return;
    }
    let start = runs[first].start.min(range.start);
    let end = runs[last - 1].end.max(range.end);
    runs.splice(first..last, std::iter::once(start..end));
}

/// Every volume being written, by download.
static VOLUMES: std::sync::LazyLock<Mutex<HashMap<i32, Vec<Arc<LiveVolume>>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Bumped on every reported write, for live extractions that poll (see [`wait_any`]).
static WRITES: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

/// Writes (and forgets) reported per download, so an extraction can tell whether anything
/// of its own download was written (see [`nzb_write_count`]).
static NZB_WRITES: std::sync::LazyLock<Mutex<HashMap<i32, u64>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn bump_nzb(nzb_id: i32) {
    *lock(&NZB_WRITES).entry(nzb_id).or_insert(0) += 1;
}

/// How many writes (and forgets) download `nzb_id` reported so far.
pub fn nzb_write_count(nzb_id: i32) -> u64 {
    lock(&NZB_WRITES).get(&nzb_id).copied().unwrap_or(0)
}

/// Where a live extraction read packed bytes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadKind {
    /// A volume still being written, as its articles landed.
    Live,
    /// A volume opened while it was written but read only after it finished: read back.
    Back,
    /// A volume that had finished before the extraction opened it.
    Finished,
}

static READS: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// Counts `n` packed bytes a live extraction read (shown in its summary line).
pub fn count_read(n: u64, kind: ReadKind) {
    READS[kind as usize].fetch_add(n, Ordering::Relaxed);
}

/// The bytes [`count_read`] counted so far, as (live, read back, finished).
pub fn read_counts() -> (u64, u64, u64) {
    let get = |k: ReadKind| READS[k as usize].load(Ordering::Relaxed);
    (get(ReadKind::Live), get(ReadKind::Back), get(ReadKind::Finished))
}

/// The summary of what an extraction read, from [`read_counts`] at its start and end.
/// `None` when nothing was read as written or read back (no volume was being written).
/// Process-wide: an extraction running beside another counts its bytes too.
pub fn read_summary(before: (u64, u64, u64), after: (u64, u64, u64)) -> Option<String> {
    if after.0 == before.0 && after.1 == before.1 {
        return None;
    }
    let mb = |a: u64, b: u64| b.saturating_sub(a) as f64 / 1_048_576.0;
    Some(format!(
        "Live extraction read {:.1} MB as written, {:.1} MB read back after their volume finished, {:.1} MB from volumes finished before",
        mb(before.0, after.0),
        mb(before.1, after.1),
        mb(before.2, after.2)
    ))
}

/// The writer opened (or created at `size` bytes) `out_path`, the direct-write output of file
/// `file_id` of download `nzb_id`, whose complete volume will be `final_path`.
/// `created`: the output was (re)created just now, zero-filled: nothing written into an
/// earlier one counts.
pub fn register(
    nzb_id: i32,
    file_id: i32,
    final_path: &[u8],
    out_path: &[u8],
    size: u64,
    created: bool,
) {
    if size == 0 || out_path.is_empty() {
        return;
    }
    let mut map = lock(&VOLUMES);
    let list = map.entry(nzb_id).or_default();
    if let Some(v) = list.iter().find(|v| v.file_id == file_id) {
        if !created && v.size == size && v.out_path.as_path() == crate::fsops::path(out_path) {
            let mut st = lock(&v.state);
            if st.final_path != final_path {
                // Renamed while downloading (direct rename, deobfuscation).
                st.final_path = final_path.to_vec();
                if is_first_volume(final_path) {
                    if st.head {
                        // Known as a set's first volume only now: its headers are there.
                        drop(st);
                        note_head(nzb_id);
                    } else {
                        drop(st);
                        v.want(0);
                    }
                }
            }
            return;
        }
        // Another output or length: what was written into the old one does not count.
        let old = v.clone();
        list.retain(|v| v.file_id != file_id);
        if let Some(w) = old.forget() {
            w.wake();
        }
    }
    let v = Arc::new(LiveVolume {
        nzb_id,
        file_id,
        out_path: crate::fsops::path(out_path).to_path_buf(),
        size,
        state: Mutex::new(State {
            final_path: final_path.to_vec(),
            ..State::default()
        }),
        read_live: AtomicU64::new(0),
    });
    // A set's first volume: the article cache writes its first article out at once (instead of
    // holding it until the volume is done), so direct unpack can start from its headers while
    // the rest downloads (see `take_new_heads`).
    if is_first_volume(final_path) {
        v.want(0);
    }
    list.push(v);
}

/// Whether `path` names the first volume of an archive set direct unpack extracts (a RAR set's
/// first volume, or the first volume of a set `crate::livefeed` reads).
fn is_first_volume(path: &[u8]) -> bool {
    let name = match path
        .iter()
        .rposition(|&b| b == b'/' || b == crate::paths::PATH_SEPARATOR)
    {
        Some(i) => &path[i + 1..],
        None => path,
    };
    crate::unpack::direct_unpack_is_main_archive(name)
}

/// Downloads with a first volume whose headers were written since [`take_new_heads`] last ran.
static NEW_HEADS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn note_head(nzb_id: i32) {
    let mut heads = lock(&NEW_HEADS);
    if !heads.contains(&nzb_id) {
        heads.push(nzb_id);
    }
}

/// Downloads in which the headers of a set's first volume were written since the last call:
/// direct unpack can start there, before the volume finishes.
pub fn take_new_heads() -> Vec<i32> {
    std::mem::take(&mut *lock(&NEW_HEADS))
}

/// The complete paths of the first volumes download `nzb_id` is writing whose headers are
/// written: direct unpack extracts them while they download.
pub fn heads(nzb_id: i32) -> Vec<Vec<u8>> {
    let map = lock(&VOLUMES);
    let Some(list) = map.get(&nzb_id) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|v| {
            let st = lock(&v.state);
            (st.head && !st.forgotten && is_first_volume(&st.final_path)).then(|| st.final_path.clone())
        })
        .collect()
}

/// The file holding the headers of the volume that will be `final_path`, while it is still
/// being written (its first bytes are): readers of a volume's headers (its signature, its
/// members' names and times) read them there before the volume is complete. `None` when no
/// download writes it, or two do.
pub fn head_path(final_path: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    let key = {
        use std::os::unix::ffi::OsStrExt;
        final_path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let key = final_path.to_string_lossy().as_bytes().to_vec();
    let map = lock(&VOLUMES);
    let mut found = map.values().flatten().filter(|v| {
        let st = lock(&v.state);
        st.final_path == key && st.head && !st.forgotten
    });
    let first = found.next()?.out_path.clone();
    found.next().is_none().then_some(first)
}

/// The writer wrote bytes `offset..offset + len` of file `file_id` into its direct-write
/// output (the write returned).
pub fn written(nzb_id: i32, file_id: i32, offset: u64, len: u64) {
    if len == 0 {
        return;
    }
    let volume = {
        let map = lock(&VOLUMES);
        map.get(&nzb_id)
            .and_then(|l| l.iter().find(|v| v.file_id == file_id).cloned())
    };
    let Some(v) = volume else {
        return;
    };
    let end = offset.saturating_add(len).min(v.size);
    if offset >= end {
        return;
    }
    let (waker, first_head) = v.written(offset..end);
    if let Some(w) = waker {
        w.wake();
    }
    if first_head {
        note_head(nzb_id);
    }
    bump_nzb(nzb_id);
    let (m, cv) = &WRITES;
    *lock(m) += 1;
    cv.notify_all();
}

/// The volume download `nzb_id` is writing that will be `final_path` once complete.
/// `None` also when two files of the download are written under that name (copies with one
/// name: which one ends up as it is not known yet).
pub fn lookup(nzb_id: i32, final_path: &[u8]) -> Option<Arc<LiveVolume>> {
    let map = lock(&VOLUMES);
    let mut found = map.get(&nzb_id)?.iter().filter(|v| lock(&v.state).final_path == final_path);
    let first = found.next()?.clone();
    found.next().is_none().then_some(first)
}

/// Download `nzb_id` writes no volume any more (it finished, moved or was deleted): readers
/// parked on its volumes are woken to find them finished or gone.
pub fn forget(nzb_id: i32) {
    crate::liveorder::clear(nzb_id);
    let gone = lock(&VOLUMES).remove(&nzb_id).unwrap_or_default();
    for v in gone {
        debug_assert_eq!(v.nzb_id, nzb_id);
        if let Some(w) = v.forget() {
            w.wake();
        }
    }
    bump_nzb(nzb_id);
    let (m, cv) = &WRITES;
    *lock(m) += 1;
    cv.notify_all();
}

/// Whether a live extraction waits for an article of file `file_id` of download `nzb_id`
/// that is not written yet: the article cache then writes that file's articles out at once
/// instead of holding them until the file is done.
pub fn wanted(nzb_id: i32, file_id: i32) -> bool {
    let map = lock(&VOLUMES);
    map.get(&nzb_id)
        .and_then(|l| l.iter().find(|v| v.file_id == file_id))
        .is_some_and(|v| lock(&v.state).waiting.is_some())
}

/// Whether a live extraction waits for any article not written yet (see [`wanted`]).
pub fn any_wanted() -> bool {
    let map = lock(&VOLUMES);
    map.values()
        .flatten()
        .any(|v| lock(&v.state).waiting.is_some())
}

/// The current write count, for [`wait_any`].
pub fn write_count() -> u64 {
    *lock(&WRITES.0)
}

/// Waits at most `timeout` for a write reported after `seen` (a [`write_count`]).
pub fn wait_any(seen: u64, timeout: Duration) {
    let (m, cv) = &WRITES;
    let guard = lock(m);
    if *guard != seen {
        return;
    }
    let _ = cv.wait_timeout(guard, timeout);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A weak fleet primary that fails over stops its direct unpack and forgets its live
    /// volumes (queuecoord's weak-primary switch): a volume its reader wanted no longer keeps
    /// the article cache writing early, and nothing of that download stays registered.
    #[test]
    fn forgetting_a_download_drops_its_wanted_volumes() {
        let nzb_id = 0x7fff_0724;
        register(nzb_id, 1, b"/x/fwp.part01.rar", b"/x/fwp.part01.rar.tmp", 1000, true);
        let v = lookup(nzb_id, b"/x/fwp.part01.rar").expect("registered");
        v.want(0);
        assert!(wanted(nzb_id, 1), "the reader waits for its first article");
        forget(nzb_id);
        assert!(!wanted(nzb_id, 1), "forgotten: the cache holds articles again");
        assert!(lookup(nzb_id, b"/x/fwp.part01.rar").is_none());
        v.want(0);
        assert!(!wanted(nzb_id, 1), "a stale handle cannot ask again");
    }

    #[test]
    fn written_ranges_merge() {
        let mut r = Vec::new();
        insert(&mut r, 10..20);
        insert(&mut r, 30..40);
        insert(&mut r, 0..5);
        assert_eq!(r, vec![0..5, 10..20, 30..40]);
        insert(&mut r, 20..30);
        assert_eq!(r, vec![0..5, 10..40]);
        insert(&mut r, 3..12);
        assert_eq!(r, vec![0..40]);
        insert(&mut r, 50..50);
        assert_eq!(r, vec![0..40]);
        assert_eq!(run_holding(&r, 39), Some(&(0..40)));
        assert_eq!(run_holding(&r, 40), None);
    }

    #[test]
    fn a_parked_reader_wakes_only_for_its_offset() {
        use std::sync::atomic::AtomicUsize;
        struct Count(AtomicUsize);
        impl std::task::Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let id = -7_310_001;
        register(id, 1, b"/d/a.part01.rar", b"/d/1.out.tmp", 1000, true);
        let v = lookup(id, b"/d/a.part01.rar").unwrap();
        assert_eq!(v.size(), 1000);
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        assert!(v.wait_for(100, &waker));
        written(id, 1, 200, 100);
        assert_eq!(count.0.load(Ordering::SeqCst), 0);
        written(id, 1, 0, 150);
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert_eq!(v.written_end(120), Some(150));
        assert!(!v.wait_for(120, &waker));
        // Past the volume's length is cut off.
        written(id, 1, 900, 500);
        assert_eq!(v.written_end(950), Some(1000));
        let mut files = zipfast::map::VolumeFiles::new();
        let mut synced = 0;
        v.sync(&mut files, 0, &mut synced);
        assert_eq!(files.written(0), vec![0..150, 200..300, 900..1000]);
        written(id, 1, 150, 50);
        v.sync(&mut files, 0, &mut synced);
        assert_eq!(files.written(0), vec![0..300, 900..1000]);
        // A rename moves the lookup to the new name.
        register(id, 1, b"/d/b.part01.rar", b"/d/1.out.tmp", 1000, false);
        assert!(lookup(id, b"/d/a.part01.rar").is_none());
        assert!(lookup(id, b"/d/b.part01.rar").is_some());
        assert!(v.wait_for(500, &waker));
        forget(id);
        assert_eq!(count.0.load(Ordering::SeqCst), 2);
        assert!(lookup(id, b"/d/b.part01.rar").is_none());
        assert!(!v.wait_for(500, &waker));
    }

    #[test]
    fn a_recreated_output_forgets_what_was_written_into_the_old_one() {
        let id = -7_310_002;
        register(id, 1, b"/d/c.rar", b"/d/1.out.tmp", 1000, true);
        written(id, 1, 0, 500);
        let old = lookup(id, b"/d/c.rar").unwrap();
        assert_eq!(old.written_end(0), Some(500));
        // The same output opened again keeps its writes; created again, it does not.
        register(id, 1, b"/d/c.rar", b"/d/1.out.tmp", 1000, false);
        assert_eq!(lookup(id, b"/d/c.rar").unwrap().written_end(0), Some(500));
        register(id, 1, b"/d/c.rar", b"/d/1.out.tmp", 1000, true);
        let new = lookup(id, b"/d/c.rar").unwrap();
        assert_eq!(new.written_end(0), None);
        assert!(!old.wait_for(600, &Waker::noop().clone()), "the old one is dropped");
        forget(id);
    }

    #[test]
    fn a_reader_that_stops_waiting_releases_the_cache() {
        let id = -7_310_003;
        register(id, 4, b"/d/d.r00", b"/d/4.out.tmp", 1000, true);
        let v = lookup(id, b"/d/d.r00").unwrap();
        assert!(!wanted(id, 4));
        v.want(100);
        assert!(wanted(id, 4) && v.is_waited());
        v.stop_waiting();
        assert!(!wanted(id, 4));
        assert!(v.wait_for(100, &Waker::noop().clone()));
        assert!(wanted(id, 4));
        written(id, 4, 0, 200);
        assert!(!wanted(id, 4), "the write it waited for releases it");
        forget(id);
    }

    #[test]
    fn a_first_volume_whose_headers_are_written_starts_direct_unpack() {
        let id = -7_310_004;
        // A first volume is wanted from the start: the cache writes its first article out.
        register(id, 1, b"/d/Rel.part01.rar", b"/d/1.out.tmp", 1000, true);
        register(id, 2, b"/d/Rel.part02.rar", b"/d/2.out.tmp", 1000, true);
        assert!(wanted(id, 1) && !wanted(id, 2));
        assert!(heads(id).is_empty());
        assert!(head_path(Path::new("/d/Rel.part01.rar")).is_none());
        written(id, 2, 0, 100);
        written(id, 1, 100, 100);
        assert!(!take_new_heads().contains(&id), "not the first volume's offset 0");
        written(id, 1, 0, 100);
        assert!(!wanted(id, 1));
        assert!(take_new_heads().contains(&id));
        assert!(!take_new_heads().contains(&id), "once");
        written(id, 1, 0, 300);
        assert!(!take_new_heads().contains(&id));
        assert_eq!(heads(id), vec![b"/d/Rel.part01.rar".to_vec()]);
        assert_eq!(head_path(Path::new("/d/Rel.part01.rar")), Some(PathBuf::from("/d/1.out.tmp")));
        // Named a first volume only by a later rename: noted then.
        register(id, 3, b"/d/abc123", b"/d/3.out.tmp", 1000, true);
        written(id, 3, 0, 100);
        assert!(!take_new_heads().contains(&id));
        register(id, 3, b"/d/Other.7z.001", b"/d/3.out.tmp", 1000, false);
        assert!(take_new_heads().contains(&id));
        assert_eq!(heads(id).len(), 2);
        forget(id);
        assert!(heads(id).is_empty());
        assert!(head_path(Path::new("/d/Rel.part01.rar")).is_none());
    }
}
