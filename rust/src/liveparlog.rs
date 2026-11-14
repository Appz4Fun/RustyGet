//! The "Live par2 for ..." progress line printed while a direct unpack waits
//! for repair. It used to be printed after every completed file; now it is
//! printed when something a reader acts on changes (damage found, recovery
//! blocks arriving or requested, repair becoming possible), when the check
//! has moved on by a tenth of the set, or once a minute while it progresses.
//! A count that goes down (the session was rebuilt) is said so explicitly.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The numbers the line shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub verified: usize,
    pub damaged: usize,
    pub not_arrived: usize,
    pub total: usize,
    pub recovery_ready: usize,
    pub recovery_in_flight: i64,
    pub recovery_needed: i64,
    pub repair_possible: bool,
}

#[derive(Clone, Copy, Debug)]
struct Mark {
    at: Instant,
    shown: Snapshot,
}

/// Why a line is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    First,
    Changed,
    Progress,
    Restarted,
}

const PROGRESS_STEP_DIVISOR: usize = 10;
const INTERVAL: Duration = Duration::from_secs(60);

/// Decides whether a line is due for `cur`, given what was shown last.
pub fn due(prev: Option<(Instant, Snapshot)>, now: Instant, cur: &Snapshot) -> Option<Due> {
    let Some((at, shown)) = prev else {
        return Some(Due::First);
    };
    if cur.verified < shown.verified || cur.total != shown.total {
        return Some(Due::Restarted);
    }
    if cur.damaged != shown.damaged
        || cur.recovery_ready != shown.recovery_ready
        || cur.recovery_in_flight != shown.recovery_in_flight
        || cur.recovery_needed != shown.recovery_needed
        || cur.repair_possible != shown.repair_possible
    {
        return Some(Due::Changed);
    }
    let step = (cur.total / PROGRESS_STEP_DIVISOR).max(1);
    if cur.verified >= shown.verified + step {
        return Some(Due::Progress);
    }
    if cur.verified != shown.verified && now.duration_since(at) >= INTERVAL {
        return Some(Due::Progress);
    }
    None
}

/// The text after "Live par2 for <name>".
pub fn format(cur: &Snapshot, why: Due) -> String {
    let mut s = format!(
        ": checked {} of {} slice(s), {} damaged, {} not downloaded yet; recovery blocks: {} downloaded, {} downloading",
        cur.verified, cur.total, cur.damaged, cur.not_arrived, cur.recovery_ready, cur.recovery_in_flight
    );
    if cur.recovery_needed > 0 {
        s.push_str(&format!(", {} more needed", cur.recovery_needed));
    }
    if cur.repair_possible {
        s.push_str("; repair possible now");
    }
    if why == Due::Restarted {
        s.push_str(" (live par2 session was rebuilt, counts start over)");
    }
    s
}

static MARKS: Mutex<Option<HashMap<i32, Mark>>> = Mutex::new(None);
const MAX_TRACKED: usize = 4096;

/// Returns the line tail to print for `nzb_id`, or `None` when nothing worth
/// showing changed since the last line.
pub fn line_if_due(nzb_id: i32, cur: Snapshot) -> Option<String> {
    let now = Instant::now();
    let mut guard = MARKS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let prev = map.get(&nzb_id).map(|m| (m.at, m.shown));
    let why = due(prev, now, &cur)?;
    if map.len() >= MAX_TRACKED && !map.contains_key(&nzb_id) {
        map.clear();
    }
    map.insert(
        nzb_id,
        Mark {
            at: now,
            shown: cur,
        },
    );
    Some(format(&cur, why))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(verified: usize) -> Snapshot {
        Snapshot {
            verified,
            damaged: 0,
            not_arrived: 1975 - verified,
            total: 1975,
            recovery_ready: 0,
            recovery_in_flight: 1,
            recovery_needed: 0,
            repair_possible: false,
        }
    }

    #[test]
    fn first_line_is_always_due() {
        assert_eq!(due(None, Instant::now(), &snap(0)), Some(Due::First));
    }

    #[test]
    fn small_progress_within_a_minute_is_quiet() {
        let t0 = Instant::now();
        let prev = Some((t0, snap(0)));
        assert_eq!(due(prev, t0 + Duration::from_secs(5), &snap(35)), None);
        assert_eq!(due(prev, t0 + Duration::from_secs(30), &snap(150)), None);
    }

    #[test]
    fn a_tenth_of_the_set_is_due() {
        let t0 = Instant::now();
        assert_eq!(
            due(Some((t0, snap(0))), t0 + Duration::from_secs(5), &snap(197)),
            Some(Due::Progress)
        );
    }

    #[test]
    fn a_minute_of_progress_is_due_but_no_progress_is_not() {
        let t0 = Instant::now();
        let prev = Some((t0, snap(100)));
        assert_eq!(
            due(prev, t0 + Duration::from_secs(61), &snap(101)),
            Some(Due::Progress)
        );
        assert_eq!(due(prev, t0 + Duration::from_secs(600), &snap(100)), None);
    }

    #[test]
    fn damage_and_recovery_changes_are_due() {
        let t0 = Instant::now();
        let prev = Some((t0, snap(100)));
        let mut damaged = snap(100);
        damaged.damaged = 2;
        assert_eq!(due(prev, t0, &damaged), Some(Due::Changed));
        let mut ready = snap(100);
        ready.recovery_ready = 1;
        assert_eq!(due(prev, t0, &ready), Some(Due::Changed));
        let mut possible = snap(100);
        possible.repair_possible = true;
        assert_eq!(due(prev, t0, &possible), Some(Due::Changed));
    }

    #[test]
    fn a_count_going_down_says_the_session_was_rebuilt() {
        let t0 = Instant::now();
        let why = due(Some((t0, snap(400))), t0, &snap(0));
        assert_eq!(why, Some(Due::Restarted));
        assert!(format(&snap(0), Due::Restarted).contains("rebuilt"));
    }

    #[test]
    fn line_names_downloaded_and_downloading_recovery_blocks() {
        let line = format(&snap(432), Due::Progress);
        assert_eq!(
            line,
            ": checked 432 of 1975 slice(s), 0 damaged, 1543 not downloaded yet; recovery blocks: 0 downloaded, 1 downloading"
        );
        let mut need = snap(432);
        need.recovery_needed = 3;
        need.repair_possible = true;
        let line = format(&need, Due::Changed);
        assert!(
            line.ends_with(", 3 more needed; repair possible now"),
            "{line}"
        );
    }

    #[test]
    fn line_if_due_suppresses_repeats_per_nzb() {
        let id = -424_242;
        assert!(line_if_due(id, snap(10)).is_some());
        assert!(line_if_due(id, snap(11)).is_none());
        assert!(line_if_due(id - 1, snap(11)).is_some());
    }
}
