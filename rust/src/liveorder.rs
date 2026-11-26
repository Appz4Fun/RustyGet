//! Download order for a set extracted live (direct unpack reading the volumes as they are
//! written, see `crate::livewrite`).
//!
//! The queue hands out articles in file-list order, which for a live set is usually its volume
//! order already. What kept the extraction behind the download on a fast line was not the order
//! but the slowest articles: about one in twenty-five takes 5 to 30 s while the rest take one
//! or two (a news server warming up its upstream connections, or routing an article to a slow
//! backend). The extraction reads the set front to back, so it stops at the first article that
//! is not written yet, and every volume finished only with its slowest article: on a 7 x 500 MB
//! set all volumes finished within 10 s of each other at the end of a 38 s download, and the
//! extraction, which started only then, ran 36 s after the last article.
//!
//! Once direct unpack knows the set, it registers the volumes' extraction order here
//! ([`set_order`]). The queue then:
//! - hands out the articles the extraction waits for before anything else ([`urge`]: the
//!   directory at the end of a 7z or ZIP set's last volume, which its map needs first);
//! - takes the set's volumes in extraction order, preferring the lowest unfinished volume and
//!   the next ones within [`WINDOW`] over the rest of the download (each volume still uses
//!   every connection, so the line rate holds; past the window the usual order goes on);
//! - restarts an article of the window that runs much longer than the others
//!   ([`hedge_threshold`]): asked again on another connection it comes in a second, where the
//!   slow request would hold the extraction (and the end of its volume) for half a minute.
//!
//! EXTRA_PRIORITY is not used: it raises the whole download above the queue, bypasses holds and
//! is saved with the queue.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

/// Volumes past the lowest unfinished one that are taken in extraction order before the rest.
pub const WINDOW: usize = 3;
/// An article of the window is restarted after running at least this long ...
pub const HEDGE_MIN_MS: u64 = 3_000;
/// ... and this many times the median time articles took lately.
pub const HEDGE_FACTOR: u64 = 3;
/// Restarts of one article at most (a slow server stays slow for it).
pub const MAX_RESTARTS: u32 = 2;
/// Article times needed before anything is restarted: without them a slow line (where every
/// article takes long) is not told from a slow article.
const MIN_SAMPLES: usize = 16;
/// Article times the median is taken over.
const SAMPLES: usize = 512;
/// Byte offsets asked for at most, per download.
const MAX_URGENT: usize = 16;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn basename(path: &[u8]) -> &[u8] {
    match path
        .iter()
        .rposition(|&b| b == b'/' || b == crate::paths::PATH_SEPARATOR)
    {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// Each volume's place in the extraction order, by file name (case ignored).
#[derive(Debug, Default)]
pub struct Ranks(HashMap<Vec<u8>, usize>);

impl Ranks {
    /// The place of the file named `filename` (a path or a bare name), if it is a volume.
    pub fn of(&self, filename: &[u8]) -> Option<usize> {
        self.0
            .get(&basename(filename).to_ascii_lowercase())
            .copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Bytes the extraction waits for before it can go on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Urgent {
    /// The article holding byte `offset` of the file named `name`.
    At { name: Vec<u8>, offset: u64 },
    /// The whole file: its next articles, in order.
    Whole { name: Vec<u8> },
}

impl Urgent {
    pub fn name(&self) -> &[u8] {
        match self {
            Urgent::At { name, .. } | Urgent::Whole { name } => name,
        }
    }
}

#[derive(Default)]
struct Order {
    ranks: Arc<Ranks>,
    urgent: Vec<Urgent>,
    /// Restarts so far, by article.
    restarts: HashMap<usize, u32>,
}

static ORDERS: LazyLock<Mutex<HashMap<i32, Order>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Whether any download has an order: the queue's hot path looks only at this otherwise.
static ANY: AtomicBool = AtomicBool::new(false);

/// Download `nzb_id` extracts the set `volumes` (names, in extraction order) while it downloads.
pub fn set_order(nzb_id: i32, volumes: &[Vec<u8>]) {
    let ranks = Ranks(
        volumes
            .iter()
            .enumerate()
            .map(|(i, v)| (basename(v).to_ascii_lowercase(), i))
            .collect(),
    );
    let mut map = lock(&ORDERS);
    map.entry(nzb_id).or_default().ranks = Arc::new(ranks);
    ANY.store(true, Ordering::Release);
}

/// Download `nzb_id` is no longer extracted live (done, declined, finished or deleted).
pub fn clear(nzb_id: i32) {
    let mut map = lock(&ORDERS);
    map.remove(&nzb_id);
    ANY.store(!map.is_empty(), Ordering::Release);
}

/// Whether any download is extracted live in order.
pub fn any() -> bool {
    ANY.load(Ordering::Acquire)
}

/// The extraction order of download `nzb_id`.
pub fn ranks(nzb_id: i32) -> Option<Arc<Ranks>> {
    if !any() {
        return None;
    }
    lock(&ORDERS)
        .get(&nzb_id)
        .map(|o| o.ranks.clone())
        .filter(|r| !r.is_empty())
}

fn push_urgent(nzb_id: i32, u: Urgent) {
    let mut map = lock(&ORDERS);
    let order = map.entry(nzb_id).or_default();
    if order.urgent.contains(&u) {
        return;
    }
    if order.urgent.len() >= MAX_URGENT {
        order.urgent.remove(0);
    }
    order.urgent.push(u);
    ANY.store(true, Ordering::Release);
}

/// The extraction of download `nzb_id` waits for byte `offset` of the file named `name`
/// (a header it must read first): its article is handed out before anything else.
pub fn urge(nzb_id: i32, name: &[u8], offset: u64) {
    push_urgent(
        nzb_id,
        Urgent::At {
            name: basename(name).to_vec(),
            offset,
        },
    );
}

/// The extraction of download `nzb_id` needs the whole file named `name` first (its headers
/// could not be found by offset).
pub fn urge_whole(nzb_id: i32, name: &[u8]) {
    push_urgent(
        nzb_id,
        Urgent::Whole {
            name: basename(name).to_vec(),
        },
    );
}

/// What the extraction of download `nzb_id` waits for.
pub fn urgent(nzb_id: i32) -> Vec<Urgent> {
    if !any() {
        return Vec::new();
    }
    lock(&ORDERS)
        .get(&nzb_id)
        .map(|o| o.urgent.clone())
        .unwrap_or_default()
}

/// `u` is handed out (or no longer needed).
pub fn done_urgent(nzb_id: i32, u: &Urgent) {
    if let Some(order) = lock(&ORDERS).get_mut(&nzb_id) {
        order.urgent.retain(|x| x != u);
    }
}

/// Counts a restart of article `article` of download `nzb_id`; false when it was restarted
/// [`MAX_RESTARTS`] times already (it is not restarted again).
pub fn note_restart(nzb_id: i32, article: usize) -> bool {
    let mut map = lock(&ORDERS);
    let Some(order) = map.get_mut(&nzb_id) else {
        return false;
    };
    let n = order.restarts.entry(article).or_insert(0);
    if *n >= MAX_RESTARTS {
        return false;
    }
    *n += 1;
    true
}

/// Whether the volume at `rank` is taken in extraction order, `lowest` being the lowest
/// unfinished volume's rank.
pub fn in_window(rank: usize, lowest: usize) -> bool {
    rank >= lowest && rank < lowest.saturating_add(WINDOW)
}

/// Milliseconds since the first call (a monotonic clock for article times).
pub fn now_ms() -> u64 {
    static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);
    EPOCH.elapsed().as_millis() as u64
}

static DURATIONS: Mutex<(Vec<u32>, usize)> = Mutex::new((Vec::new(), 0));

/// An article took `ms` from its request to its last byte.
pub fn note_duration(ms: u64) {
    let mut d = lock(&DURATIONS);
    let v = ms.min(u64::from(u32::MAX)) as u32;
    if d.0.len() < SAMPLES {
        d.0.push(v);
    } else {
        let at = d.1;
        d.0[at] = v;
    }
    d.1 = (d.1 + 1) % SAMPLES;
}

/// The median of the article times noted lately; `None` before there are enough.
pub fn median_ms() -> Option<u64> {
    let d = lock(&DURATIONS);
    if d.0.len() < MIN_SAMPLES {
        return None;
    }
    let mut v = d.0.clone();
    drop(d);
    let mid = v.len() / 2;
    let (_, m, _) = v.select_nth_unstable(mid);
    Some(u64::from(*m))
}

/// How long an article of the window may run before it is asked again elsewhere.
pub fn hedge_threshold(median_ms: u64) -> u64 {
    HEDGE_MIN_MS.max(median_ms.saturating_mul(HEDGE_FACTOR))
}

/// The article of a file of `count` articles and `total` bytes that holds byte `offset`.
/// `known` are the (offset, size) of articles already decoded, by article index (`None`: not
/// yet). yEnc posts cut a file into parts of one size, so one known part places them all.
pub fn article_for(offset: u64, known: &[Option<(u64, u64)>], count: usize, total: u64) -> usize {
    if count == 0 {
        return 0;
    }
    let last = count - 1;
    if let Some(i) = known
        .iter()
        .position(|k| k.is_some_and(|(o, s)| s > 0 && o <= offset && offset < o + s))
    {
        return i;
    }
    // The first part's length is every part's but the last.
    if let Some((i, (o, s))) = known
        .iter()
        .enumerate()
        .find_map(|(i, k)| k.filter(|&(_, s)| s > 0).map(|k| (i, k)))
    {
        let part = if i < last {
            s
        } else {
            o.checked_div(i as u64).unwrap_or(s)
        };
        if part > 0 {
            return ((offset / part) as usize).min(last);
        }
    }
    if total == 0 {
        return last;
    }
    ((u128::from(offset) * count as u128 / u128::from(total)) as usize).min(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_follow_the_extraction_order_by_name() {
        let id = -7_320_001;
        set_order(
            id,
            &[
                b"Movie.7z.001".to_vec(),
                b"Movie.7z.002".to_vec(),
                b"Movie.7z.003".to_vec(),
            ],
        );
        let r = ranks(id).unwrap();
        assert_eq!(r.of(b"/dl/movie.7Z.002"), Some(1));
        assert_eq!(r.of(b"Movie.7z.003"), Some(2));
        assert_eq!(r.of(b"Movie.par2"), None);
        assert!(any());
        clear(id);
        assert!(ranks(id).is_none());
    }

    #[test]
    fn the_window_runs_from_the_lowest_unfinished_volume() {
        assert!(in_window(4, 4));
        assert!(in_window(6, 4));
        assert!(!in_window(7, 4));
        assert!(
            !in_window(3, 4),
            "a volume below the lowest unfinished one is finished"
        );
    }

    #[test]
    fn urgent_offsets_are_kept_once_until_handed_out() {
        let id = -7_320_002;
        urge(id, b"/dl/Movie.7z.007", 177_000_000);
        urge(id, b"Movie.7z.007", 177_000_000);
        urge_whole(id, b"Movie.7z.007");
        let u = urgent(id);
        assert_eq!(u.len(), 2);
        assert_eq!(
            u[0],
            Urgent::At {
                name: b"Movie.7z.007".to_vec(),
                offset: 177_000_000
            }
        );
        done_urgent(id, &u[0]);
        assert_eq!(
            urgent(id),
            vec![Urgent::Whole {
                name: b"Movie.7z.007".to_vec()
            }]
        );
        for i in 0..40 {
            urge(id, b"a.zip", i);
        }
        assert_eq!(urgent(id).len(), MAX_URGENT);
        clear(id);
        assert!(urgent(id).is_empty());
    }

    #[test]
    fn an_article_is_restarted_at_most_twice() {
        let id = -7_320_003;
        assert!(!note_restart(id, 1), "no order, no restart");
        set_order(id, &[b"a.part01.rar".to_vec()]);
        assert!(note_restart(id, 1));
        assert!(note_restart(id, 1));
        assert!(!note_restart(id, 1));
        assert!(note_restart(id, 2));
        clear(id);
    }

    #[test]
    fn the_threshold_follows_the_median_with_a_floor() {
        assert_eq!(hedge_threshold(300), HEDGE_MIN_MS);
        assert_eq!(hedge_threshold(1_300), 3_900);
        assert_eq!(hedge_threshold(4_000), 12_000);
    }

    #[test]
    fn the_median_needs_enough_samples() {
        // (process-wide: other tests may add samples; only the floor is checked)
        for _ in 0..MIN_SAMPLES {
            note_duration(1_000);
        }
        assert!(median_ms().is_some());
    }

    #[test]
    fn an_offset_is_placed_in_its_article() {
        let mb = 1_000_000u64;
        // Nothing decoded yet: by proportion.
        assert_eq!(article_for(0, &[None; 10], 10, 10 * mb), 0);
        assert_eq!(article_for(10 * mb - 1, &[None; 10], 10, 10 * mb), 9);
        assert_eq!(article_for(5 * mb, &[None; 10], 10, 10 * mb), 5);
        // One part decoded: every part has its size.
        let mut known = vec![None; 10];
        known[0] = Some((0, 700_000));
        assert_eq!(article_for(1_400_001, &known, 10, 9_500_000), 2);
        assert_eq!(article_for(9_400_000, &known, 10, 9_500_000), 9);
        // The article holding it is decoded: that one.
        known[3] = Some((2_100_000, 700_000));
        assert_eq!(article_for(2_100_000, &known, 10, 9_500_000), 3);
        // Only the last part known: the parts before it share its offset.
        let mut tail = vec![None; 10];
        tail[9] = Some((6_300_000, 200_000));
        assert_eq!(article_for(1_400_001, &tail, 10, 6_500_000), 2);
        assert_eq!(article_for(5, &[], 0, 0), 0);
    }
}
