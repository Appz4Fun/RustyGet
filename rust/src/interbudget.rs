//! The InterDir disk budget: a download that has not started yet starts only when the
//! file system it downloads into (InterDir, or DestDir when InterDir is empty) has room
//! for what it still has to fetch, after what the downloads already started will still
//! write, and after `DiskSpace` and a reserve for the queue's state files.
//!
//! 6790, 2026-11-27: about 450 GB of downloads started together in a 470 GB InterDir; the
//! disk filled up, the state saves failed and a partial state was lost. `DiskSpace` alone
//! pauses all downloading once the disk is already nearly full, by which time every started
//! download is stuck half done. The budget keeps the next one from starting instead.

/// MB kept free for the queue's state files (they live with the downloads on 6790), on top
/// of `DiskSpace`. Test hook: `RUSTYGET_TEST_STATE_RESERVE_MB`.
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

/// The room a download still needs: its unpaused remaining bytes plus the margin.
pub(crate) fn need_bytes(remaining: i64, paused: i64) -> i64 {
    let left = remaining.saturating_sub(paused).max(0);
    left.saturating_mul(MARGIN_PERCENT) / 100
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

/// The budget as it stood at the last check, recomputed at most once per second.
#[derive(Default)]
struct Budget {
    /// The second it was computed; 0: never.
    sec: i64,
    /// Off: `DiskSpace` <= 0, no directory, or its free space unknown.
    on: bool,
    avail: i64,
    /// Still to be written by started downloads, plus the downloads admitted since `sec`.
    reserved: i64,
    keep_mb: i64,
    /// Downloads admitted since `sec`: not counted as started yet, but already reserved.
    admitted: Vec<i32>,
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

/// One queue item as the budget sees it.
pub(crate) struct Item {
    pub id: i32,
    pub remaining: i64,
    pub paused: i64,
    pub started: bool,
}

/// Free bytes of the budget's directory and the MB to keep free, from the live options and
/// the file system; None: the budget is off.
fn probe() -> Option<(i64, i64)> {
    let (dir, disk_space) = inputs()?;
    let (avail, _) = crate::fsops::get_disk_state(&dir)?;
    Some((avail, disk_space.saturating_add(state_reserve_mb())))
}

/// Recomputes the budget when a second has passed since the last time. `probe` and `items`
/// are called only then, so a pick reads the queue's sizes once per second, not per pick.
fn refresh(
    b: &mut Budget,
    now: i64,
    probe: &mut dyn FnMut() -> Option<(i64, i64)>,
    items: &mut dyn FnMut() -> Vec<Item>,
) {
    if b.sec != 0 && now == b.sec {
        return;
    }
    b.sec = now;
    b.admitted.clear();
    let Some((avail, keep_mb)) = probe() else {
        b.on = false;
        return;
    };
    b.on = true;
    b.avail = avail;
    b.keep_mb = keep_mb;
    let all = items();
    b.reserved = all
        .iter()
        .filter(|i| i.started)
        .map(|i| need_bytes(i.remaining, i.paused))
        .fold(0i64, i64::saturating_add);
    // An item that is gone or has started is no longer held: log it again if it is held
    // again later.
    b.logged.retain(|id| all.iter().any(|i| i.id == *id && !i.started));
}

/// The outcome of a budget check for one unstarted download.
pub(crate) enum Verdict {
    Start,
    /// Held; `Some((need_mb, free_mb))` the first time, to be logged.
    Hold(Option<(i64, i64)>),
}

/// Whether the unstarted download `id` (needing `need` bytes) may start now. Admitting it
/// reserves its room at once, so a second unstarted download picked within the same second
/// does not count on the same free space.
pub(crate) fn check(now: i64, id: i32, need: i64, items: &mut dyn FnMut() -> Vec<Item>) -> Verdict {
    let mut guard = BUDGET.lock().unwrap_or_else(|e| e.into_inner());
    let b = guard.get_or_insert_with(Budget::default);
    check_with(b, now, id, need, &mut probe, items)
}

fn check_with(
    b: &mut Budget,
    now: i64,
    id: i32,
    need: i64,
    probe: &mut dyn FnMut() -> Option<(i64, i64)>,
    items: &mut dyn FnMut() -> Vec<Item>,
) -> Verdict {
    refresh(b, now, probe, items);
    if !b.on || b.admitted.contains(&id) {
        return Verdict::Start;
    }
    if fits(b.avail, b.reserved, need, b.keep_mb) {
        b.reserved = b.reserved.saturating_add(need);
        b.admitted.push(id);
        b.logged.remove(&id);
        return Verdict::Start;
    }
    if b.logged.insert(id) {
        let free = b.avail.saturating_sub(b.reserved).max(0);
        let mb = |v: i64| v / (1024 * 1024);
        return Verdict::Hold(Some((mb(need) + b.keep_mb, mb(free))));
    }
    Verdict::Hold(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: i64 = 1024 * 1024;

    #[test]
    fn need_counts_unpaused_remaining_plus_ten_percent() {
        assert_eq!(need_bytes(100 * MB, 0), 110 * MB);
        assert_eq!(need_bytes(100 * MB, 40 * MB), 66 * MB);
        assert_eq!(need_bytes(10, 20), 0);
        assert_eq!(need_bytes(i64::MAX, 0), i64::MAX / 100);
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
        Item { id, remaining: remaining_mb * MB, paused: 0, started }
    }

    /// The scenario's numbers: a 64 MB file system, A and B 30 MB each, 5 MB to keep.
    #[test]
    fn a_second_download_waits_until_the_first_is_done() {
        let mut b = Budget::default();
        let mut probe = || Some((60 * MB, 5));
        let need = need_bytes(30 * MB, 0);
        // second 1: neither started; A is admitted, B (same second) counts A's room
        let mut q = || vec![item(1, 30, false), item(2, 30, false)];
        assert!(matches!(check_with(&mut b, 1, 1, need, &mut probe, &mut q), Verdict::Start));
        assert!(matches!(check_with(&mut b, 1, 1, need, &mut probe, &mut q), Verdict::Start));
        assert!(matches!(
            check_with(&mut b, 1, 2, need, &mut probe, &mut q),
            Verdict::Hold(Some((38, 27)))
        ));
        // logged once
        assert!(matches!(check_with(&mut b, 1, 2, need, &mut probe, &mut q), Verdict::Hold(None)));
        // second 2: A started, half done; still no room for B
        let mut q = || vec![item(1, 15, true), item(2, 30, false)];
        let mut probe2 = || Some((45 * MB, 5));
        assert!(matches!(check_with(&mut b, 2, 2, need, &mut probe2, &mut q), Verdict::Hold(None)));
        // second 3: A done and gone from the queue: B starts
        let mut q = || vec![item(2, 30, false)];
        let mut probe3 = || Some((30 * MB + 33 * MB, 5));
        assert!(matches!(check_with(&mut b, 3, 2, need, &mut probe3, &mut q), Verdict::Start));
        assert!(b.logged.is_empty());
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
        assert!(matches!(check_with(&mut b, 9, 1, i64::MAX, &mut probe, &mut q), Verdict::Start));
        assert_eq!(calls, 0);
        let mut probe = || Some((MB, 0));
        let mut q = || {
            calls += 1;
            vec![item(1, 1000, true)]
        };
        for _ in 0..5 {
            assert!(matches!(check_with(&mut b, 10, 2, MB, &mut probe, &mut q), Verdict::Hold(_)));
        }
        assert_eq!(calls, 1);
    }

    #[test]
    fn a_hold_is_logged_again_after_the_item_left_the_queue() {
        let mut b = Budget::default();
        let mut probe = || Some((MB, 0));
        let mut q = || vec![item(5, 100, false)];
        let need = need_bytes(100 * MB, 0);
        assert!(matches!(check_with(&mut b, 1, 5, need, &mut probe, &mut q), Verdict::Hold(Some(_))));
        let mut gone = || Vec::new();
        assert!(matches!(check_with(&mut b, 2, 6, MB * 2, &mut probe, &mut gone), Verdict::Hold(Some(_))));
        assert!(!b.logged.contains(&5));
    }
}
