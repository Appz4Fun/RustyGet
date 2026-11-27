//! The InterDir disk budget: a download that has not started yet starts only when the
//! file system it downloads into (InterDir, or DestDir when InterDir is empty) has room
//! for what it still has to fetch and unpack, after what the downloads already started will
//! still write, and after `DiskSpace` and a reserve for the queue's state files.
//!
//! 6790, 2026-11-27: about 450 GB of downloads started together in a 470 GB InterDir; the
//! disk filled up, the state saves failed and a partial state was lost. `DiskSpace` alone
//! pauses all downloading once the disk is already nearly full, by which time every started
//! download is stuck half done. The budget keeps the next one from starting instead.
//!
//! With InterDir empty the global DestDir is measured; a category DestDir on another file
//! system is not (DiskSpace still pauses there). `DiskSpace` <= 0 turns the budget off.

/// MB kept free on top of `DiskSpace`: the queue's state files (they live with the downloads
/// on 6790) and the articles the article cache holds but has not written yet. Test hook:
/// `RUSTYGET_TEST_STATE_RESERVE_MB`.
const STATE_RESERVE_MB: i64 = 1024;

/// What is still to be written counts 10 % more (yEnc overhead is decoded away, but par2
/// repair, renames and the partial state files of a download take room as well).
const MARGIN_PERCENT: i64 = 110;

fn state_reserve_mb() -> i64 {
    static MB: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *MB.get_or_init(|| {
        std::env::var("RUSTYGET_TEST_STATE_RESERVE_MB")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .filter(|v: &i64| *v >= 0)
            .unwrap_or(STATE_RESERVE_MB)
    })
}

/// The room a download still needs: its unpaused remaining bytes plus the margin, plus
/// `unpack` bytes its unpack will still write (next to the archives, before they are
/// cleaned up; 0 when it does not unpack or has unpacked already).
pub(crate) fn need_bytes(remaining: i64, paused: i64, unpack: i64) -> i64 {
    let left = remaining.saturating_sub(paused).max(0);
    (left.saturating_mul(MARGIN_PERCENT) / 100).saturating_add(unpack.max(0))
}

/// Whether a download has started: some of it came (or failed), or an article is in flight.
pub(crate) fn started(success: i64, failed: i64, active_downloads: i32) -> bool {
    success.saturating_add(failed) > 0 || active_downloads > 0
}

/// Whether an unstarted download that needs `need` bytes may start: `avail` bytes are free,
/// `reserved` of them are still to be written by the downloads already started, and
/// `keep_mb` MB must stay free.
pub(crate) fn fits(avail: i64, reserved: i64, need: i64, keep_mb: i64) -> bool {
    let keep = keep_mb.saturating_mul(1024 * 1024);
    avail.saturating_sub(reserved).saturating_sub(need) >= keep
}

/// A download's place as the pick orders it: extra priority first, then priority, then
/// queue position (earlier ranks higher).
pub(crate) type Rank = (bool, i32, std::cmp::Reverse<usize>);

/// The budget as it stood at the last check, recomputed at most once per second.
#[derive(Default)]
struct Budget {
    /// The second it was computed; 0: never.
    sec: i64,
    /// Off: `DiskSpace` <= 0, no directory, or its free space unknown.
    on: bool,
    avail: i64,
    total: i64,
    /// Still to be written by started downloads, plus the downloads admitted since `sec`.
    reserved: i64,
    keep_mb: i64,
    /// Downloads admitted since `sec`: not counted as started yet, but already reserved.
    admitted: Vec<i32>,
    /// Downloads held since `sec`: free space and reservations cannot improve within the
    /// second, so the pick skips them without asking again.
    held: std::collections::HashSet<i32>,
    /// The highest rank held since `sec` of a download that fits the free space as it is
    /// and only waits for the started downloads' room: unstarted downloads ranked below it
    /// (lower priority, or the same priority later in the queue) wait too, so smaller ones
    /// never take its room. One that does not fit even the free space holds nobody.
    hold_below: Option<Rank>,
    /// Downloads whose hold has been logged (once each, until admitted).
    logged: std::collections::HashSet<i32>,
}

static BUDGET: std::sync::Mutex<Option<Budget>> = std::sync::Mutex::new(None);

/// The directory the budget measures and `DiskSpace`, from the live options; None: off.
fn inputs() -> Option<(std::ffi::CString, i64)> {
    use crate::options::OptStr;
    let o = crate::options::active_options()?;
    let disk_space = i64::from(o.scalars().disk_space);
    if disk_space <= 0 {
        return None;
    }
    let inter = o.str_bytes(OptStr::InterDir);
    let dir = if inter.is_empty() {
        o.str_bytes(OptStr::DestDir)
    } else {
        inter
    };
    if dir.is_empty() {
        return None;
    }
    let dir = dir.split(|&b| b == 0).next().unwrap_or(&[]);
    Some((std::ffi::CString::new(dir).ok()?, disk_space))
}

/// Whether the unpack of downloads is on (option `Unpack`).
pub(crate) fn unpack_on() -> bool {
    crate::options::active_options().is_some_and(|o| o.scalars().unpack != 0)
}

/// One queue item as the budget sees it.
pub(crate) struct Item {
    pub id: i32,
    pub started: bool,
    /// What it will still write when it counts (started, and not held back as a duplicate
    /// copy of another download); 0 otherwise.
    pub reserve: i64,
}

/// The download a pick would start.
pub(crate) struct Pick {
    pub id: i32,
    pub need: i64,
    pub rank: Rank,
    /// FORCE priority: it starts regardless, but its room is reserved all the same.
    pub force: bool,
}

/// Free and total bytes of the budget's directory and the MB to keep free, from the live
/// options and the file system; None: the budget is off (logged once when the directory
/// cannot be measured).
fn probe() -> Option<(i64, i64, i64)> {
    static UNMEASURED_LOGGED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    let (dir, disk_space) = inputs()?;
    let Some((avail, total)) = crate::fsops::get_disk_state(&dir) else {
        // Logged once per failure spell: cleared again when the directory reads.
        if !UNMEASURED_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let mut msg = b"InterDir disk budget off: cannot read the free space of ".to_vec();
            msg.extend_from_slice(dir.as_bytes());
            crate::log::log_active_bytes(crate::log::MK_WARNING, &msg);
        }
        return None;
    };
    UNMEASURED_LOGGED.store(false, std::sync::atomic::Ordering::Relaxed);
    Some((avail, total, disk_space.saturating_add(state_reserve_mb())))
}

/// Recomputes the budget when a second has passed since the last time. `probe` and `items`
/// are called only then, so a pick reads the queue's sizes once per second, not per pick.
fn refresh(
    b: &mut Budget,
    now: i64,
    probe: &mut dyn FnMut() -> Option<(i64, i64, i64)>,
    items: &mut dyn FnMut() -> Vec<Item>,
) {
    if b.sec != 0 && now == b.sec {
        return;
    }
    b.sec = now;
    b.admitted.clear();
    b.held.clear();
    b.hold_below = None;
    let Some((avail, total, keep_mb)) = probe() else {
        b.on = false;
        return;
    };
    b.on = true;
    b.avail = avail;
    b.total = total;
    b.keep_mb = keep_mb;
    let all = items();
    b.reserved = all
        .iter()
        .filter(|i| i.started)
        .map(|i| i.reserve)
        .fold(0i64, i64::saturating_add);
    // An item that is gone or has started is no longer held: log it again if it is held
    // again later.
    if !b.logged.is_empty() {
        let waiting: std::collections::HashSet<i32> =
            all.iter().filter(|i| !i.started).map(|i| i.id).collect();
        b.logged.retain(|id| waiting.contains(id));
    }
}

/// Why a download is held, the first time (to be logged).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HoldLog {
    pub need_mb: i64,
    pub free_mb: i64,
    /// It needs more than the whole file system: it never starts by itself.
    pub never_fits: bool,
}

/// The outcome of a budget check for one unstarted download.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Start,
    /// Held; `Some` the first time, to be logged.
    Hold(Option<HoldLog>),
}

/// Whether the unstarted download `pick` may start now. Admitting it reserves its room at
/// once, so a second unstarted download picked within the same second does not count on the
/// same free space.
pub(crate) fn check(now: i64, pick: &Pick, items: &mut dyn FnMut() -> Vec<Item>) -> Verdict {
    let mut guard = BUDGET.lock().unwrap_or_else(|e| e.into_inner());
    let b = guard.get_or_insert_with(Budget::default);
    check_with(b, now, pick, &mut probe, items)
}

/// Whether the pick's walk can skip the unstarted download `id` of rank `rank` at once: it
/// was held this second, or a download ranked above it was.
pub(crate) fn held_now(now: i64, id: i32, rank: Rank) -> bool {
    let guard = BUDGET.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().is_some_and(|b| held_with(b, now, id, rank))
}

fn held_with(b: &Budget, now: i64, id: i32, rank: Rank) -> bool {
    b.on && b.sec == now && (b.held.contains(&id) || b.hold_below.is_some_and(|r| rank < r))
}

fn check_with(
    b: &mut Budget,
    now: i64,
    pick: &Pick,
    probe: &mut dyn FnMut() -> Option<(i64, i64, i64)>,
    items: &mut dyn FnMut() -> Vec<Item>,
) -> Verdict {
    refresh(b, now, probe, items);
    if !b.on || b.admitted.contains(&pick.id) {
        return Verdict::Start;
    }
    let below = b.hold_below.is_some_and(|r| pick.rank < r);
    if pick.force || (!below && fits(b.avail, b.reserved, pick.need, b.keep_mb)) {
        b.reserved = b.reserved.saturating_add(pick.need);
        b.admitted.push(pick.id);
        b.logged.remove(&pick.id);
        return Verdict::Start;
    }
    b.held.insert(pick.id);
    let never_fits = pick
        .need
        .saturating_add(b.keep_mb.saturating_mul(1024 * 1024))
        > b.total;
    // Waiting for the started downloads' room only (it fits the free space as it is): the
    // ones ranked below it wait as well. Otherwise it may never fit (the disk is taken by
    // data that stays) and must not hold the queue.
    let waits_for_room = fits(b.avail, 0, pick.need, b.keep_mb);
    if waits_for_room && b.hold_below.is_none_or(|r| pick.rank > r) {
        b.hold_below = Some(pick.rank);
    }
    if b.logged.insert(pick.id) {
        // Free: what the started downloads leave (DiskSpace and the reserve not taken off;
        // they are in the need).
        let free = b.avail.saturating_sub(b.reserved).max(0);
        let mb = |v: i64| v / (1024 * 1024);
        return Verdict::Hold(Some(HoldLog {
            need_mb: mb(pick.need) + b.keep_mb,
            free_mb: mb(free),
            never_fits,
        }));
    }
    Verdict::Hold(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: i64 = 1024 * 1024;

    #[test]
    fn need_counts_unpaused_remaining_plus_ten_percent_and_the_unpack() {
        assert_eq!(need_bytes(100 * MB, 0, 0), 110 * MB);
        assert_eq!(need_bytes(100 * MB, 40 * MB, 0), 66 * MB);
        assert_eq!(need_bytes(10, 20, 0), 0);
        assert_eq!(need_bytes(i64::MAX, 0, 0), i64::MAX / 100);
        // downloaded, unpack still to come: its extracted size
        assert_eq!(need_bytes(0, 0, 90 * MB), 90 * MB);
        assert_eq!(need_bytes(100 * MB, 0, 90 * MB), 200 * MB);
        assert_eq!(need_bytes(i64::MAX, 0, i64::MAX), i64::MAX);
    }

    #[test]
    fn started_means_bytes_came_or_an_article_is_in_flight() {
        assert!(!started(0, 0, 0));
        assert!(started(1, 0, 0));
        assert!(started(0, 1, 0));
        assert!(started(0, 0, 1));
    }

    #[test]
    fn fits_keeps_disk_space_and_reserve_free_after_started_downloads() {
        // 64 MB free, 33 MB reserved by A, B needs 33 MB, 5 MB to keep: no
        assert!(!fits(64 * MB, 33 * MB, 33 * MB, 5));
        // A finished: nothing reserved
        assert!(fits(64 * MB, 0, 33 * MB, 5));
        // exactly at the line
        assert!(fits(10 * MB, 2 * MB, 3 * MB, 5));
        assert!(!fits(10 * MB, 2 * MB, 3 * MB + 1, 5));
        // started downloads already over budget: nothing new starts
        assert!(!fits(10 * MB, 20 * MB, 0, 0));
        assert!(!fits(i64::MIN, i64::MAX, i64::MAX, i64::MAX));
    }

    fn item(id: i32, remaining_mb: i64, started: bool) -> Item {
        Item {
            id,
            started,
            reserve: need_bytes(remaining_mb * MB, 0, 0),
        }
    }

    fn r(extra: bool, priority: i32, position: usize) -> Rank {
        (extra, priority, std::cmp::Reverse(position))
    }

    fn pick(id: i32, need: i64) -> Pick {
        Pick {
            id,
            need,
            rank: r(false, 0, 0),
            force: false,
        }
    }

    fn hold_log(v: &Verdict) -> Option<&HoldLog> {
        match v {
            Verdict::Hold(Some(log)) => Some(log),
            _ => None,
        }
    }

    /// The scenario's numbers: a 64 MB file system, A and B 30 MB each, 5 MB to keep.
    #[test]
    fn a_second_download_waits_until_the_first_is_done() {
        let mut b = Budget::default();
        let mut probe = || Some((60 * MB, 64 * MB, 5));
        let need = need_bytes(30 * MB, 0, 0);
        // second 1: neither started; A is admitted, B (same second) counts A's room
        let mut q = || vec![item(1, 30, false), item(2, 30, false)];
        assert_eq!(
            check_with(&mut b, 1, &pick(1, need), &mut probe, &mut q),
            Verdict::Start
        );
        assert_eq!(
            check_with(&mut b, 1, &pick(1, need), &mut probe, &mut q),
            Verdict::Start
        );
        assert_eq!(
            check_with(&mut b, 1, &pick(2, need), &mut probe, &mut q),
            Verdict::Hold(Some(HoldLog {
                need_mb: 38,
                free_mb: 27,
                never_fits: false
            }))
        );
        assert!(held_with(&b, 1, 2, r(false, 0, 0)));
        assert!(
            !held_with(&b, 2, 2, r(false, 0, 0)),
            "a new second judges again"
        );
        // logged once
        assert_eq!(
            check_with(&mut b, 1, &pick(2, need), &mut probe, &mut q),
            Verdict::Hold(None)
        );
        // second 2: A started, half done; still no room for B
        let mut q = || vec![item(1, 15, true), item(2, 30, false)];
        let mut probe2 = || Some((45 * MB, 64 * MB, 5));
        assert_eq!(
            check_with(&mut b, 2, &pick(2, need), &mut probe2, &mut q),
            Verdict::Hold(None)
        );
        // second 3: A done and gone from the queue: B starts
        let mut q = || vec![item(2, 30, false)];
        let mut probe3 = || Some((30 * MB + 33 * MB, 64 * MB, 5));
        assert_eq!(
            check_with(&mut b, 3, &pick(2, need), &mut probe3, &mut q),
            Verdict::Start
        );
        assert!(b.logged.is_empty());
    }

    #[test]
    fn a_downloaded_item_still_to_unpack_keeps_its_room() {
        let mut b = Budget::default();
        // A: fully downloaded (remaining 0), 30 MB to extract next to its archives
        let mut probe = || Some((60 * MB, 128 * MB, 5));
        let mut q = || {
            vec![
                Item {
                    id: 1,
                    started: true,
                    reserve: need_bytes(0, 0, 30 * MB),
                },
                item(2, 30, false),
            ]
        };
        assert!(hold_log(&check_with(
            &mut b,
            1,
            &pick(2, need_bytes(30 * MB, 0, 0)),
            &mut probe,
            &mut q
        ))
        .is_some());
    }

    #[test]
    fn a_held_download_holds_lower_ranked_ones_but_not_higher_or_started() {
        let mut b = Budget::default();
        // 50 MB of the 100 MB free are reserved by a started download
        let mut probe = || Some((100 * MB, 200 * MB, 0));
        let mut q = || {
            vec![Item {
                id: 9,
                started: true,
                reserve: 50 * MB,
            }]
        };
        let big = Pick {
            id: 1,
            need: 60 * MB,
            rank: r(false, 50, 0),
            force: false,
        };
        assert!(hold_log(&check_with(&mut b, 1, &big, &mut probe, &mut q)).is_some());
        // a small one of lower priority does not take its room
        let small_low = Pick {
            id: 2,
            need: MB,
            rank: r(false, 0, 0),
            force: false,
        };
        assert!(held_with(&b, 1, 2, small_low.rank));
        assert!(matches!(
            check_with(&mut b, 1, &small_low, &mut probe, &mut q),
            Verdict::Hold(_)
        ));
        // a higher priority still may
        let small_same = Pick {
            id: 3,
            need: MB,
            rank: r(false, 60, 3),
            force: false,
        };
        assert_eq!(
            check_with(&mut b, 1, &small_same, &mut probe, &mut q),
            Verdict::Start
        );
        let small_extra = Pick {
            id: 4,
            need: MB,
            rank: r(true, -100, 0),
            force: false,
        };
        assert_eq!(
            check_with(&mut b, 1, &small_extra, &mut probe, &mut q),
            Verdict::Start
        );
    }

    #[test]
    fn one_larger_than_the_disk_says_so_and_holds_nobody_else() {
        let mut b = Budget::default();
        let mut probe = || Some((90 * MB, 100 * MB, 0));
        let mut q = || Vec::new();
        let huge = Pick {
            id: 1,
            need: 200 * MB,
            rank: r(false, 50, 0),
            force: false,
        };
        let log = check_with(&mut b, 1, &huge, &mut probe, &mut q);
        assert!(hold_log(&log).is_some_and(|l| l.never_fits));
        let small_low = Pick {
            id: 2,
            need: MB,
            rank: r(false, 0, 0),
            force: false,
        };
        assert_eq!(
            check_with(&mut b, 1, &small_low, &mut probe, &mut q),
            Verdict::Start
        );
    }

    #[test]
    fn force_starts_but_reserves_its_room() {
        let mut b = Budget::default();
        let mut probe = || Some((50 * MB, 100 * MB, 0));
        let mut q = || Vec::new();
        let forced = Pick {
            id: 1,
            need: 60 * MB,
            rank: r(false, 900, 0),
            force: true,
        };
        assert_eq!(
            check_with(&mut b, 1, &forced, &mut probe, &mut q),
            Verdict::Start
        );
        assert!(matches!(
            check_with(&mut b, 1, &pick(2, MB), &mut probe, &mut q),
            Verdict::Hold(_)
        ));
    }

    #[test]
    fn off_without_disk_space_or_free_space_and_reads_the_queue_once_per_second() {
        let mut b = Budget::default();
        let mut calls = 0;
        let mut probe = || None;
        let mut q = || {
            calls += 1;
            Vec::new()
        };
        assert_eq!(
            check_with(&mut b, 9, &pick(1, i64::MAX), &mut probe, &mut q),
            Verdict::Start
        );
        assert!(!held_with(&b, 9, 1, r(false, 0, 0)));
        assert_eq!(calls, 0);
        let mut probe = || Some((MB, 100 * MB, 0));
        let mut q = || {
            calls += 1;
            vec![item(1, 1000, true)]
        };
        for _ in 0..5 {
            assert!(matches!(
                check_with(&mut b, 10, &pick(2, MB), &mut probe, &mut q),
                Verdict::Hold(_)
            ));
        }
        assert_eq!(calls, 1);
    }

    #[test]
    fn a_hold_is_logged_again_after_the_item_left_the_queue() {
        let mut b = Budget::default();
        let mut probe = || Some((MB, 1000 * MB, 0));
        let mut q = || vec![item(5, 100, false)];
        let need = need_bytes(100 * MB, 0, 0);
        assert!(hold_log(&check_with(&mut b, 1, &pick(5, need), &mut probe, &mut q)).is_some());
        let mut gone = || Vec::new();
        assert!(hold_log(&check_with(
            &mut b,
            2,
            &pick(6, MB * 2),
            &mut probe,
            &mut gone
        ))
        .is_some());
        assert!(!b.logged.contains(&5));
    }

    #[test]
    fn the_same_priority_later_in_the_queue_waits_behind_a_held_download() {
        let mut b = Budget::default();
        let mut probe = || Some((70 * MB, 100 * MB, 0));
        let mut q = || vec![item(9, 30, true)];
        // 33 MB reserved by a started one: the first in the queue (60 MB) waits for its room
        let first = Pick {
            id: 1,
            need: 60 * MB,
            rank: r(false, 0, 0),
            force: false,
        };
        assert!(hold_log(&check_with(&mut b, 1, &first, &mut probe, &mut q)).is_some());
        let later = Pick {
            id: 2,
            need: MB,
            rank: r(false, 0, 5),
            force: false,
        };
        assert!(held_with(&b, 1, 2, later.rank));
        assert!(matches!(
            check_with(&mut b, 1, &later, &mut probe, &mut q),
            Verdict::Hold(_)
        ));
        let earlier_higher = Pick {
            id: 3,
            need: MB,
            rank: r(false, 10, 7),
            force: false,
        };
        assert_eq!(
            check_with(&mut b, 1, &earlier_higher, &mut probe, &mut q),
            Verdict::Start
        );
    }

    #[test]
    fn one_that_does_not_fit_the_free_space_holds_nobody() {
        // a large disk mostly taken by data that stays: total says it fits, free space never
        let mut b = Budget::default();
        let mut probe = || Some((10 * MB, 1000 * MB, 0));
        let mut q = || Vec::new();
        let big = Pick {
            id: 1,
            need: 50 * MB,
            rank: r(false, 50, 0),
            force: false,
        };
        let log = check_with(&mut b, 1, &big, &mut probe, &mut q);
        assert!(hold_log(&log).is_some_and(|l| !l.never_fits));
        let small = Pick {
            id: 2,
            need: MB,
            rank: r(false, 0, 1),
            force: false,
        };
        assert!(!held_with(&b, 1, 2, small.rank));
        assert_eq!(
            check_with(&mut b, 1, &small, &mut probe, &mut q),
            Verdict::Start
        );
    }
}
