//! End-to-end Rust implementation of `DupeProbe` (`daemon/queue/DupeProbe.cpp`):
//! even-spread sample indexing (`sample_indexes`), NNTP `STAT` reply classification
//! (`classify`), dead-posting verdict (`is_dead`), server group/level ordering
//! (`select_server_order`), and probe/recheck outcome evaluation and log formatting
//! (`eval_run_outcome`, `eval_recheck_outcome`).

pub const DUPE_PROBE_EXISTS: i32 = 0;
pub const DUPE_PROBE_MISSING: i32 = 1;
pub const DUPE_PROBE_UNKNOWN: i32 = 2;

pub const SAMPLE_COUNT: i32 = 10;
pub const MIN_ARTICLES: i32 = 4;
pub const MIN_ALIVE_SAMPLES: i32 = 2;
pub const RECENT_BLOCK_SEC: i64 = 60;

/// Computes `DupeProbe::SampleIndexes(total, count)`.
pub fn sample_indexes(total: usize, count: i32) -> Vec<usize> {
    if total == 0 || count <= 0 {
        return Vec::new();
    }
    let wanted = total.min(count as usize);
    let mut indexes = Vec::with_capacity(wanted);
    for i in 0..wanted {
        let num = (2 * i + 1) as u128 * (total as u128);
        let den = (2 * wanted) as u128;
        indexes.push((num / den) as usize);
    }
    indexes
}

/// Classifies an NNTP `STAT` response for `DupeProbe::Classify`.
pub fn classify(response: Option<&[u8]>) -> i32 {
    let Some(resp) = response else {
        return DUPE_PROBE_UNKNOWN;
    };
    if resp.starts_with(b"2") {
        return DUPE_PROBE_EXISTS;
    }
    if resp.starts_with(b"400") || resp.starts_with(b"499") {
        return DUPE_PROBE_UNKNOWN;
    }
    if resp.starts_with(b"41")
        || resp.starts_with(b"42")
        || resp.starts_with(b"43")
        || resp.starts_with(b"451")
    {
        return DUPE_PROBE_MISSING;
    }
    DUPE_PROBE_UNKNOWN
}

/// Computes `DupeProbe::IsDead(existing, definitive_servers, active_servers)`.
pub fn is_dead(existing: i32, definitive_servers: i32, active_servers: i32) -> bool {
    existing < MIN_ALIVE_SAMPLES && definitive_servers > 0 && definitive_servers >= active_servers
}

/// Selects and sorts active servers for `DupeProbe::Measure`:
/// filters to `active[i] != 0`, deduplicates positive `groups[i]` in iteration order,
/// sorts by `(norm_levels[i], ids[i])`, and returns the sorted server `ids`.
pub fn select_server_order(
    active: &[u8],
    groups: &[i32],
    norm_levels: &[i32],
    ids: &[i32],
) -> Vec<i32> {
    let n = active
        .len()
        .min(groups.len())
        .min(norm_levels.len())
        .min(ids.len());
    let mut seen_groups: Vec<i32> = Vec::new();
    let mut order: Vec<(i32, i32)> = Vec::new();
    for i in 0..n {
        if active[i] == 0 {
            continue;
        }
        let g = groups[i];
        if g > 0 {
            if seen_groups.contains(&g) {
                continue;
            }
            seen_groups.push(g);
        }
        order.push((norm_levels[i], ids[i]));
    }
    order.sort_unstable();
    order.into_iter().map(|(_, id)| id).collect()
}

/// Evaluates the outcome and formats the log message for `DupeProbe::Run`.
/// Returns `(outcome, message)`:
/// - `1`: dead posting (`Note(mkInfo, msg)` + `Abandon`)
/// - `2`: alive posting (`Note(mkInfo, msg)`)
/// - `3`: no server reached (`SetDeadPickProbed(false)` + `PrintMessage(mkDetail, msg)`)
/// - `4`: inconclusive verdict (`Note(mkInfo, msg)`)
/// - `0`: unfinished / cancelled
#[allow(clippy::too_many_arguments)]
pub fn eval_run_outcome(
    existing: i32,
    sampled: i32,
    missing_servers: i32,
    active_servers: i32,
    unreachable_servers: i32,
    reached_servers: i32,
    finished: bool,
    found_on: &[u8],
) -> (i32, Vec<u8>) {
    let dead = finished && is_dead(existing, missing_servers, active_servers);
    let unreachable_suffix = if unreachable_servers > 0 {
        format!(", {unreachable_servers} unreachable left out")
    } else {
        String::new()
    };

    if dead {
        let mut out = format!(
            "Dupe probe: {existing} of {sampled} sampled articles exist ({missing_servers} of {active_servers} servers answered definitively{unreachable_suffix})"
        )
        .into_bytes();
        if !found_on.is_empty() {
            out.extend_from_slice(b", on ");
            out.extend_from_slice(found_on);
        }
        out.extend_from_slice(b": the posting is dead");
        (1, out)
    } else if existing >= MIN_ALIVE_SAMPLES {
        let mut out = format!(
            "Dupe probe: {existing} of {sampled} sampled articles found (it stops at {MIN_ALIVE_SAMPLES}), the first on "
        )
        .into_bytes();
        out.extend_from_slice(found_on);
        out.extend_from_slice(b": not abandoning it");
        (2, out)
    } else if finished && reached_servers == 0 {
        (
            3,
            b"Dupe probe: no server could be asked, will probe again".to_vec(),
        )
    } else if finished {
        let out = format!(
            "Dupe probe: no verdict ({existing} of {sampled} sampled articles exist, {missing_servers} of {active_servers} servers answered definitively{unreachable_suffix})"
        )
        .into_bytes();
        (4, out)
    } else {
        (0, Vec::new())
    }
}

/// Evaluates the outcome and formats the log message for `DupeProbe::Recheck`.
/// Returns `(outcome, message)`:
/// - `1`: no server reached (`PrintMessage(mkInfo, msg)` + clear `*DupeRecheck`)
/// - `2`: duplicate queued in place (`PrintMessage(mkInfo, msg)`)
/// - `3`: retry failed articles (`PrintMessage(mkInfo, msg)` + `eaHistoryRetryFailed`)
/// - `4`: do not retry (`PrintMessage(mkInfo, msg)`)
pub fn eval_recheck_outcome(
    existing: i32,
    sampled: i32,
    reached_servers: i32,
    duplicate_queued_name: &[u8],
) -> (i32, Vec<u8>) {
    if reached_servers == 0 {
        return (
            1,
            b"Failed articles not rechecked: no server connection was free".to_vec(),
        );
    }
    let retry = existing.saturating_mul(2) >= sampled;
    if retry && !duplicate_queued_name.is_empty() {
        let mut out = b"Not retrying failed articles: duplicate ".to_vec();
        out.extend_from_slice(duplicate_queued_name);
        out.extend_from_slice(b" downloads in its place");
        return (2, out);
    }
    let suffix = if retry {
        ": retrying failed articles"
    } else {
        "; not retrying"
    };
    let out = format!("{existing} of {sampled} failed articles exist on the servers{suffix}")
        .into_bytes();
    (if retry { 3 } else { 4 }, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_indexes_and_classify() {
        assert_eq!(sample_indexes(100, 4), vec![12, 37, 62, 87]);
        assert_eq!(classify(Some(b"223 0 <msg@id>")), DUPE_PROBE_EXISTS);
        assert_eq!(classify(Some(b"430 No such article")), DUPE_PROBE_MISSING);
        assert_eq!(classify(Some(b"400 Service busy")), DUPE_PROBE_UNKNOWN);
        assert!(is_dead(1, 3, 3));
        assert!(!is_dead(2, 3, 3));
        assert!(!is_dead(0, 2, 3));
    }

    #[test]
    fn run_and_recheck_outcomes() {
        let (code, msg) = eval_run_outcome(0, 10, 3, 3, 1, 3, true, b"");
        assert_eq!(code, 1);
        assert_eq!(
            msg,
            b"Dupe probe: 0 of 10 sampled articles exist (3 of 3 servers answered definitively, 1 unreachable left out): the posting is dead"
        );

        let (code, msg) = eval_recheck_outcome(6, 10, 2, b"");
        assert_eq!(code, 3);
        assert_eq!(
            msg,
            b"6 of 10 failed articles exist on the servers: retrying failed articles"
        );
    }

    #[test]
    fn dupe_probe_full_suite_port_from_cpp() {
        // 1. DupeProbeSampleIndexesTest
        let indexes = sample_indexes(100, 10);
        assert_eq!(indexes.len(), 10);
        assert_eq!(*indexes.first().unwrap(), 5);
        assert_eq!(*indexes.last().unwrap(), 95);
        for i in 1..indexes.len() {
            assert_eq!(indexes[i] - indexes[i - 1], 10);
        }

        let indexes4 = sample_indexes(4, 10);
        assert_eq!(indexes4.len(), 4);
        assert_eq!(indexes4[0], 0);
        assert_eq!(indexes4[3], 3);

        for total in 1..300 {
            let idx = sample_indexes(total, 10);
            for i in 0..idx.len() {
                assert!(idx[i] < total);
                assert!(i == 0 || idx[i] > idx[i - 1]);
            }
        }
        assert!(sample_indexes(0, 10).is_empty());
        assert!(sample_indexes(10, 0).is_empty());

        // 2. DupeProbeClassifyTest
        assert_eq!(classify(Some(b"223 0 <id> article exists")), DUPE_PROBE_EXISTS);
        assert_eq!(classify(Some(b"430 No Such Article Found")), DUPE_PROBE_MISSING);
        assert_eq!(classify(Some(b"423 No such article number")), DUPE_PROBE_MISSING);
        assert_eq!(classify(Some(b"412 no newsgroup selected")), DUPE_PROBE_MISSING);
        assert_eq!(classify(Some(b"451 Article not found")), DUPE_PROBE_MISSING);

        assert_eq!(classify(None), DUPE_PROBE_UNKNOWN);
        assert_eq!(
            classify(Some(b"400 service temporarily unavailable")),
            DUPE_PROBE_UNKNOWN
        );
        assert_eq!(classify(Some(b"499 connection limit")), DUPE_PROBE_UNKNOWN);
        assert_eq!(classify(Some(b"481 authentication failed")), DUPE_PROBE_UNKNOWN);
        assert_eq!(classify(Some(b"502 too many connections")), DUPE_PROBE_UNKNOWN);
        assert_eq!(classify(Some(b"500 unknown command")), DUPE_PROBE_UNKNOWN);

        // 3. DupeProbeIsDeadTest
        assert!(is_dead(0, 8, 8));
        assert!(!is_dead(0, 7, 8));
        assert!(!is_dead(0, 5, 8));

        assert!(is_dead(0, 1, 1));
        assert!(is_dead(0, 3, 3));
        assert!(!is_dead(0, 2, 3));

        assert!(is_dead(1, 8, 8));
        assert!(!is_dead(2, 8, 8));
        assert!(!is_dead(3, 5, 5));
        assert!(!is_dead(1, 4, 8));

        assert!(!is_dead(0, 0, 0));
        assert!(!is_dead(0, 0, 6));
    }
}
