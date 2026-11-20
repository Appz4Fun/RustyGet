//! DonorScore: the DupeScore offsets of duplicates (donors and backups) under a
//! pick, as daemon/dupesearch/DonorScore.cpp gave them.
//!
//! Rounding is the C++ `std::nearbyint` in the default mode: halves go to even.

/// The score of a posting known dead.
pub const DEAD: i32 = 1;
/// The highest donor offset.
pub const TOP: i32 = 90;

/// The base of the donor scores under a pick with this score.
pub fn base(pick_score: i32) -> i32 {
    pick_score.wrapping_sub(1000).max(0)
}

/// The offset for a posting alive by this share (negative: unknown, taken as
/// whole): 9 + 80 * share, one more for a byte-identical twin, at most TOP.
pub fn target(alive: f64, twin: bool) -> i32 {
    // std::min(1.0, alive): 1.0 unless alive < 1.0 (NaN keeps 1.0)
    let share = if alive < 0.0 {
        1.0
    } else if alive < 1.0 {
        alive
    } else {
        1.0
    };
    TOP.min(9 + (80.0 * share).round_ties_even() as i32 + i32::from(twin))
}

/// `DonorScore::Ranks::Take`: starts at `target(alive, twin)` and counts down
/// while the score is above `DEAD + 1` and already in `used`.
pub fn take_rank(alive: f64, twin: bool, used: &[i32]) -> i32 {
    let mut score = target(alive, twin);
    while score > DEAD + 1 && used.contains(&score) {
        score -= 1;
    }
    score
}

/// One duplicate as Rerank sees it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Entry {
    pub score: i32,
    pub alive: f64,
    pub twin: bool,
    pub bytes: i64,
    pub grabs: i32,
    /// `DonorKind` of a non-twin (`crate::donorkind::DonorKind as u8`:
    /// 1 borrowable, 2 borrowable once decrypted, 3 unknown, 4 unborrowable);
    /// a twin is always first.
    pub kind: u8,
    /// The highest offset this entry can be given (0: none). A partly alive
    /// fleet member is never raised above its old score (see
    /// `eval_member_rerank`); the entries ranked after it must then fall
    /// below that, or they would overtake it.
    pub cap: i32,
    /// Archive headers encrypted and no password (see
    /// [`crate::donorkind::LOCKED_PARAM`]): never extractable, so ranked after
    /// every entry that is, whatever its health.
    pub locked: bool,
}

impl Entry {
    /// TWINs first, then borrowable, encrypted borrowable, unknown and
    /// unborrowable ALTs.
    fn kind_rank(&self) -> u8 {
        if self.twin {
            0
        } else {
            self.kind.max(1)
        }
    }
}

/// The offsets for the live entries once every check is in, in the order a
/// fleet ranks its copies: healthy ones first (at least
/// [`crate::dupesearch_nzb::FLEET_HEALTHY_ALIVE_PERMILLE`] alive), then weak
/// ones, then those not measured, then locked ones (`Entry::locked`); within
/// each tier TWINs first, then borrowable, unknown and unborrowable ALTs
/// (`Entry::kind`); within each, most
/// alive first (whole percent), then closest to `pick_bytes`, most grabbed; strictly
/// falling, each at most its target. Entries scored DEAD (or less) keep their
/// score. Parallel to `entries`.
pub fn rerank(entries: &[Entry], pick_bytes: i64) -> Vec<i32> {
    let mut scores: Vec<i32> = entries.iter().map(|e| e.score).collect();
    let mut live: Vec<usize> = (0..entries.len())
        .filter(|&i| entries[i].score > DEAD)
        .collect();
    let percent =
        |e: &Entry| (100.0 * if e.alive < 0.0 { 1.0 } else { e.alive }).round_ties_even() as i32;
    let distance = |e: &Entry| {
        if e.bytes > pick_bytes {
            e.bytes.wrapping_sub(pick_bytes)
        } else {
            pick_bytes.wrapping_sub(e.bytes)
        }
    };
    let tier = |e: &Entry| -> u8 {
        if e.locked {
            3
        } else if e.alive < 0.0 {
            2
        } else if percent(e) * 10 < crate::dupesearch_nzb::FLEET_HEALTHY_ALIVE_PERMILLE {
            1
        } else {
            0
        }
    };
    // a stable sort keeps equal entries in order
    live.sort_by(|&a, &b| {
        let (x, y) = (&entries[a], &entries[b]);
        tier(x)
            .cmp(&tier(y))
            .then_with(|| x.kind_rank().cmp(&y.kind_rank()))
            .then_with(|| percent(y).cmp(&percent(x)))
            .then_with(|| distance(x).cmp(&distance(y)))
            .then_with(|| y.grabs.cmp(&x.grabs))
    });
    let mut previous = 100;
    for i in live {
        let mut want = target(entries[i].alive, entries[i].twin).min(previous - 1);
        if entries[i].cap > 0 {
            want = want.min(entries[i].cap);
        }
        let want = (DEAD + 1).max(want);
        previous = if entries[i].cap > 0 { want.min(entries[i].cap) } else { want };
        scores[i] = want;
    }
    scores
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(score: i32, alive: f64, twin: bool, bytes: i64, grabs: i32) -> Entry {
        Entry {
            score,
            alive,
            twin,
            bytes,
            grabs,
            kind: crate::donorkind::DonorKind::Unknown as u8,
            cap: 0,
            locked: false,
        }
    }

    #[test]
    fn a_locked_copy_goes_after_every_extractable_one() {
        // encrypted headers, no password: whole but never extractable
        let locked = Entry {
            kind: crate::donorkind::DonorKind::Unborrowable as u8,
            locked: true,
            ..e(50, 1.0, false, 1000, 0)
        };
        let weak = Entry {
            kind: 1,
            ..e(50, 0.84, false, 1000, 0)
        };
        let unmeasured = e(50, -1.0, false, 1000, 0);
        let got = rerank(&[locked, weak, unmeasured], 1000);
        assert!(got[1] > got[2] && got[2] > got[0], "{got:?}");
        // alone it still gets a live score
        let got = rerank(&[locked], 1000);
        assert!(got[0] > DEAD, "{got:?}");
    }

    #[test]
    fn a_healthy_alt_outranks_a_weak_twin() {
        // 97.5% alive or more goes first, whatever the kind
        let twin = Entry {
            kind: 0,
            ..e(50, 0.90, true, 1000, 0)
        };
        let alt = Entry {
            kind: 1,
            ..e(50, 0.99, false, 1000, 0)
        };
        let got = rerank(&[twin, alt], 1000);
        assert!(got[1] > got[0], "{got:?}");
        // both healthy: the twin first
        let twin = Entry {
            kind: 0,
            ..e(50, 0.975, true, 1000, 0)
        };
        let got = rerank(&[alt, twin], 1000);
        assert!(got[1] > got[0], "{got:?}");
        // a copy not measured comes after a weak one
        let unknown = Entry {
            kind: 1,
            ..e(50, -1.0, false, 1000, 0)
        };
        let weak = Entry {
            kind: crate::donorkind::DonorKind::Unborrowable as u8,
            ..e(50, 0.5, false, 1000, 0)
        };
        let got = rerank(&[unknown, weak], 1000);
        assert!(got[1] > got[0], "{got:?}");
    }

    #[test]
    fn a_capped_member_keeps_the_entries_after_it_below() {
        // a 98%-alive twin member scored 80 before, a whole borrowable ALT 90:
        // the twin can't rise past 80, so the ALT must drop below it
        let twin = Entry { cap: 80, ..e(DEAD + 1, 0.98, true, 1000, 0) };
        let alt = Entry { kind: 1, ..e(DEAD + 1, 1.0, false, 1000, 0) };
        let got = rerank(&[alt, twin], 1000);
        assert_eq!(got[1], 80);
        assert!(got[0] < got[1], "{got:?}");
    }

    #[test]
    fn base_never_below_zero() {
        assert_eq!(base(1_000_000), 999_000);
        assert_eq!(base(23_859_118), 23_858_118);
        assert_eq!(base(500), 0);
    }

    #[test]
    fn target_rounds_halves_to_even_and_caps() {
        assert_eq!(target(1.0, false), 89);
        assert_eq!(target(1.0, true), 90);
        assert_eq!(target(-1.0, false), 89);
        assert_eq!(target(0.5, false), 49);
        assert_eq!(target(0.0, false), 9);
        assert_eq!(target(0.95, false), 85);
        assert_eq!(target(0.9, true), 82);
        assert_eq!(target(0.0125 * 0.5, false), 9); // 80 * share = 0.5 rounds to 0
        assert_eq!(target(f64::NAN, false), 89);
    }

    #[test]
    fn ranks_are_unique_and_never_below_dead_plus_one() {
        let mut used = Vec::new();
        let s1 = take_rank(1.0, true, &used);
        used.push(s1);
        assert_eq!(s1, 90);
        let s2 = take_rank(1.0, true, &used);
        used.push(s2);
        assert_eq!(s2, 89);
        let s3 = take_rank(1.0, false, &used);
        used.push(s3);
        assert_eq!(s3, 88);
        used.retain(|&x| x != 89);
        let s4 = take_rank(1.0, false, &used);
        used.push(s4);
        assert_eq!(s4, 89);

        let mut low_used = Vec::new();
        for _ in 0..20 {
            let s = take_rank(0.0, false, &low_used);
            low_used.push(s);
            assert!(s >= DEAD + 1);
        }
    }

    #[test]
    fn rerank_orders_by_health_tier_then_kind_then_health_then_size_then_grabs() {
        let entries = [
            e(50, 0.9, false, 100, 0),
            e(50, 1.0, false, 300, 0),
            e(1, 1.0, false, 100, 0),
            e(50, 1.0, true, 300, 0),
            e(50, 1.0, false, 100, 5),
        ];
        assert_eq!(rerank(&entries, 100), vec![81, 88, 1, 90, 89]);

        let wholeness = [
            e(90, 0.9, true, 1000, 50),
            e(89, 0.95, false, 1100, 40),
            e(88, 1.0, true, 1000, 1),
            e(87, 1.0, false, 1200, 1),
        ];
        // among healthy copies a twin outranks an ALT; a healthy ALT outranks
        // a weak (90%) twin
        assert_eq!(rerank(&wholeness, 1000), vec![82, 81, 90, 89]);

        let k = |kind: u8, alive: f64| Entry {
            kind,
            ..e(50, alive, false, 1000, 0)
        };
        // healthy copies borrowable, then unknown, then unborrowable; the weak
        // (90%) borrowable one after all of them
        let kinds = [k(3, 1.0), k(2, 1.0), k(1, 0.9), k(1, 1.0), k(0, 1.0)];
        // kind 0 on a non-twin counts as borrowable
        assert_eq!(rerank(&kinds, 1000), vec![86, 87, 81, 89, 88]);

        let dead_and_falling = [
            e(1, 0.0, false, 1000, 5),
            e(50, 1.0, false, 1000, 5),
            e(49, 1.0, false, 1000, 4),
        ];
        assert_eq!(rerank(&dead_and_falling, 1000), vec![1, 89, 88]);
    }
}
