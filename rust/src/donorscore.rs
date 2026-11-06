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
    let share = if alive < 0.0 { 1.0 } else if alive < 1.0 { alive } else { 1.0 };
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
}

/// The offsets for the live entries once every check is in: most alive first
/// (whole percent), then twin, closest to `pick_bytes`, most grabbed; strictly
/// falling, each at most its target. Entries scored DEAD (or less) keep their
/// score. Parallel to `entries`.
pub fn rerank(entries: &[Entry], pick_bytes: i64) -> Vec<i32> {
    let mut scores: Vec<i32> = entries.iter().map(|e| e.score).collect();
    let mut live: Vec<usize> = (0..entries.len()).filter(|&i| entries[i].score > DEAD).collect();
    let percent = |e: &Entry| (100.0 * if e.alive < 0.0 { 1.0 } else { e.alive }).round_ties_even() as i32;
    let distance = |e: &Entry| {
        if e.bytes > pick_bytes { e.bytes.wrapping_sub(pick_bytes) } else { pick_bytes.wrapping_sub(e.bytes) }
    };
    // std::stable_sort with the C++ comparator: a stable sort keeps equal entries in order
    live.sort_by(|&a, &b| {
        let (x, y) = (&entries[a], &entries[b]);
        percent(y)
            .cmp(&percent(x))
            .then_with(|| y.twin.cmp(&x.twin))
            .then_with(|| distance(x).cmp(&distance(y)))
            .then_with(|| y.grabs.cmp(&x.grabs))
    });
    let mut previous = 100;
    for i in live {
        let want = (DEAD + 1).max(target(entries[i].alive, entries[i].twin).min(previous - 1));
        previous = want;
        scores[i] = want;
    }
    scores
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(score: i32, alive: f64, twin: bool, bytes: i64, grabs: i32) -> Entry {
        Entry { score, alive, twin, bytes, grabs }
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
    fn rerank_orders_by_health_then_twin_then_size_then_grabs() {
        let entries = [e(50, 0.9, false, 100, 0), e(50, 1.0, false, 300, 0), e(1, 1.0, false, 100, 0),
                       e(50, 1.0, true, 300, 0), e(50, 1.0, false, 100, 5)];
        assert_eq!(rerank(&entries, 100), vec![81, 88, 1, 90, 89]);

        let wholeness = [
            e(90, 0.9, true, 1000, 50),
            e(89, 0.95, false, 1100, 40),
            e(88, 1.0, true, 1000, 1),
            e(87, 1.0, false, 1200, 1),
        ];
        assert_eq!(rerank(&wholeness, 1000), vec![82, 85, 90, 89]);

        let dead_and_falling = [
            e(1, 0.0, false, 1000, 5),
            e(50, 1.0, false, 1000, 5),
            e(49, 1.0, false, 1000, 4),
        ];
        assert_eq!(rerank(&dead_and_falling, 1000), vec![1, 89, 88]);
    }
}
