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
}

#[derive(Debug, Default)]
struct State {
    /// The full path the volume will have once complete, as the download names it now.
    final_path: Vec<u8>,
    /// Byte ranges written, sorted and disjoint.
    written: Vec<Range<u64>>,
    /// A reader parked on an offset not written yet, woken once it is.
    waiting: Option<(u64, Waker)>,
    /// The download dropped this volume (finished, moved or deleted): nothing more comes.
    forgotten: bool,
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

    /// Parks `waker` until `offset` is written (or the volume is dropped). False when it is
    /// written already, or nothing more comes: the caller reads again instead of waiting.
    pub fn wait_for(&self, offset: u64, waker: &Waker) -> bool {
        let mut st = lock(&self.state);
        if st.forgotten || run_holding(&st.written, offset).is_some() {
            return false;
        }
        st.waiting = Some((offset, waker.clone()));
        true
    }

    /// Gives zipfast's writer-fed source every range written since the last call: `synced`
    /// counts the ranges already handed over (the written ranges only ever grow or merge, so
    /// a changed count, or a grown range, is new).
    pub fn sync(
        &self,
        files: &mut zipfast::map::VolumeFiles,
        volume: u32,
        synced: &mut Vec<Range<u64>>,
    ) {
        let st = lock(&self.state);
        if st.written == *synced {
            return;
        }
        for r in &st.written {
            if !synced.iter().any(|s| s.start <= r.start && r.end <= s.end) {
                files.mark_written(volume, r.clone());
            }
        }
        synced.clone_from(&st.written);
    }

    fn written(&self, range: Range<u64>) -> Option<Waker> {
        let mut st = lock(&self.state);
        insert(&mut st.written, range);
        match st.waiting.take() {
            Some((offset, waker)) if run_holding(&st.written, offset).is_some() => Some(waker),
            other => {
                st.waiting = other;
                None
            }
        }
    }

    fn forget(&self) -> Option<Waker> {
        let mut st = lock(&self.state);
        st.forgotten = true;
        st.waiting.take().map(|(_, w)| w)
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
pub fn read_summary(before: (u64, u64, u64), after: (u64, u64, u64)) -> String {
    let mb = |a: u64, b: u64| b.saturating_sub(a) as f64 / 1_048_576.0;
    format!(
        "Live extraction read {:.1} MB as written, {:.1} MB read back after their volume finished, {:.1} MB from volumes finished before",
        mb(before.0, after.0),
        mb(before.1, after.1),
        mb(before.2, after.2)
    )
}

/// The writer opened (or created at `size` bytes) `out_path`, the direct-write output of file
/// `file_id` of download `nzb_id`, whose complete volume will be `final_path`.
pub fn register(nzb_id: i32, file_id: i32, final_path: &[u8], out_path: &[u8], size: u64) {
    if size == 0 || out_path.is_empty() {
        return;
    }
    let mut map = lock(&VOLUMES);
    let list = map.entry(nzb_id).or_default();
    if let Some(v) = list.iter().find(|v| v.file_id == file_id) {
        if v.size == size && v.out_path.as_path() == crate::fsops::path(out_path) {
            let mut st = lock(&v.state);
            if st.final_path != final_path {
                // Renamed while downloading (direct rename, deobfuscation).
                st.final_path = final_path.to_vec();
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
    list.push(Arc::new(LiveVolume {
        nzb_id,
        file_id,
        out_path: crate::fsops::path(out_path).to_path_buf(),
        size,
        state: Mutex::new(State {
            final_path: final_path.to_vec(),
            ..State::default()
        }),
    }));
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
    if let Some(w) = v.written(offset..end) {
        w.wake();
    }
    let (m, cv) = &WRITES;
    *lock(m) += 1;
    cv.notify_all();
}

/// The volume download `nzb_id` is writing that will be `final_path` once complete.
pub fn lookup(nzb_id: i32, final_path: &[u8]) -> Option<Arc<LiveVolume>> {
    let map = lock(&VOLUMES);
    map.get(&nzb_id)?
        .iter()
        .find(|v| lock(&v.state).final_path == final_path)
        .cloned()
}

/// Download `nzb_id` writes no volume any more (it finished, moved or was deleted): readers
/// parked on its volumes are woken to find them finished or gone.
pub fn forget(nzb_id: i32) {
    let gone = lock(&VOLUMES).remove(&nzb_id).unwrap_or_default();
    for v in gone {
        debug_assert_eq!(v.nzb_id, nzb_id);
        if let Some(w) = v.forget() {
            w.wake();
        }
    }
    let (m, cv) = &WRITES;
    *lock(m) += 1;
    cv.notify_all();
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
        register(id, 1, b"/d/a.part01.rar", b"/d/1.out.tmp", 1000);
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
        let mut synced = Vec::new();
        v.sync(&mut files, 0, &mut synced);
        assert_eq!(files.written(0), vec![0..150, 200..300, 900..1000]);
        written(id, 1, 150, 50);
        v.sync(&mut files, 0, &mut synced);
        assert_eq!(files.written(0), vec![0..300, 900..1000]);
        // A rename moves the lookup to the new name.
        register(id, 1, b"/d/b.part01.rar", b"/d/1.out.tmp", 1000);
        assert!(lookup(id, b"/d/a.part01.rar").is_none());
        assert!(lookup(id, b"/d/b.part01.rar").is_some());
        assert!(v.wait_for(500, &waker));
        forget(id);
        assert_eq!(count.0.load(Ordering::SeqCst), 2);
        assert!(lookup(id, b"/d/b.part01.rar").is_none());
        assert!(!v.wait_for(500, &waker));
    }
}
