//! Opt-in, allocation-free samples of the complete ArticleCompleted body.
//! Each coordinator owns its counters; snapshots are emitted after downloader joins.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

static NEXT_REPORT_ID: AtomicU64 = AtomicU64::new(1);

const EXACT: usize = 128;
const BUCKETS: usize = EXACT + (64 - 7) * EXACT;

pub(crate) struct Histogram {
    report_id: u64,
    bins: [AtomicU64; BUCKETS],
    count: AtomicU64,
    invalid: AtomicBool,
}

struct Snapshot {
    count: u64,
    valid: bool,
    p99_bounds_ns: Option<(u64, u64)>,
    bins: Vec<(u64, u64, u64)>,
}

pub(crate) struct Sample<'a> {
    histogram: &'a Histogram,
    started: Instant,
}

impl Histogram {
    const fn new() -> Self {
        Self {
            report_id: 0,
            bins: [const { AtomicU64::new(0) }; BUCKETS],
            count: AtomicU64::new(0),
            invalid: AtomicBool::new(false),
        }
    }

    /// Enable only in a controlled measurement process, for both compared variants.
    pub(crate) fn configured() -> Option<Box<Self>> {
        if std::env::var_os("RUSTYGET_BENCH_CALLBACK_METRICS").as_deref()
            != Some(std::ffi::OsStr::new("1"))
        {
            return None;
        }
        let report_id = NEXT_REPORT_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .ok()?;
        let mut histogram = Box::new(Self::new());
        histogram.report_id = report_id;
        Some(histogram)
    }

    pub(crate) fn sample(&self) -> Sample<'_> {
        Sample {
            histogram: self,
            started: Instant::now(),
        }
    }

    fn record_ns(&self, elapsed: u128) {
        let Ok(elapsed) = u64::try_from(elapsed) else {
            self.invalid.store(true, Ordering::Relaxed);
            return;
        };
        let index = if elapsed < EXACT as u64 {
            elapsed as usize
        } else {
            let exponent = 63 - elapsed.leading_zeros() as usize;
            let shift = exponent - 7;
            EXACT + shift * EXACT + ((elapsed >> shift) as usize - EXACT)
        };
        for counter in [&self.bins[index], &self.count] {
            if counter
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .is_err()
            {
                self.invalid.store(true, Ordering::Relaxed);
            }
        }
    }

    fn bounds(index: usize) -> (u64, u64) {
        if index < EXACT {
            return (index as u64, index as u64);
        }
        let offset = index - EXACT;
        let shift = offset / EXACT;
        let lower = ((EXACT + offset % EXACT) as u64) << shift;
        (lower, lower + ((1u64 << shift) - 1))
    }

    // The caller must have joined every producer. A concurrent snapshot is not
    // an atomic measurement and may fail the count-consistency check.
    fn snapshot(&self) -> Snapshot {
        let count = self.count.load(Ordering::Relaxed);
        let mut total = 0u128;
        let mut bins = Vec::new();
        for (index, counter) in self.bins.iter().enumerate() {
            let n = counter.load(Ordering::Relaxed);
            if n != 0 {
                let (lower, upper) = Self::bounds(index);
                bins.push((lower, upper, n));
                total += u128::from(n);
            }
        }
        let valid =
            count != 0 && total == u128::from(count) && !self.invalid.load(Ordering::Relaxed);
        let p99_bounds_ns = if valid {
            // ceil(99*n/100), without multiplying a potentially large counter.
            let rank = u128::from(count - count / 100);
            let mut cumulative = 0u128;
            bins.iter().find_map(|&(lower, upper, n)| {
                cumulative += u128::from(n);
                (cumulative >= rank).then_some((lower, upper))
            })
        } else {
            None
        };
        Snapshot {
            count,
            valid,
            p99_bounds_ns,
            bins,
        }
    }

    /// Call only after all article-downloader callbacks have returned and joined.
    pub(crate) fn emit_after_join(&self) {
        let snapshot = self.snapshot();
        for record in snapshot.records(self.report_id) {
            crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, &record);
        }
    }
}

impl Snapshot {
    // Each message fits the existing native logger's 1,023-byte DETAIL limit.
    // CRC covers a canonical LE-u64 preamble and every indexed sparse row;
    // missing, duplicate or mixed-lifetime records invalidate the controller packet.
    fn records(&self, report_id: u64) -> impl Iterator<Item = Vec<u8>> + '_ {
        let rows = self.bins.len() as u64;
        let mut crc = crc32fast::Hasher::new();
        for value in [report_id, self.count, rows] {
            crc.update(&value.to_le_bytes());
        }
        for (index, &(lower, upper, count)) in self.bins.iter().enumerate() {
            for value in [index as u64, lower, upper, count] {
                crc.update(&value.to_le_bytes());
            }
        }
        let checksum = crc.finalize();
        let bins = self.bins.iter().enumerate().map(move |(index, &(lower, upper, count))| {
            format!("ArticleCompleted bin {{\"version\":1,\"report_id\":{report_id},\"index\":{index},\"rows\":{rows},\"lower_ns\":{lower},\"upper_ns\":{upper},\"count\":{count}}}").into_bytes()
        });
        bins.chain(std::iter::once_with(move || {
            let (lower, upper) = self.p99_bounds_ns.map_or_else(
                || ("null".to_owned(), "null".to_owned()),
                |(lower, upper)| (lower.to_string(), upper.to_string()),
            );
            format!("ArticleCompleted metrics {{\"version\":1,\"report_id\":{report_id},\"boundary\":\"queuecoord.article_completed\",\"scope\":\"coordinator_lifetime\",\"quiescent\":true,\"valid\":{},\"callbacks\":{},\"p99_lower_ns\":{lower},\"p99_upper_ns\":{upper},\"histogram_rows\":{rows},\"histogram_crc32\":{checksum},\"histogram_method\":\"loglinear128_ns_v1\",\"transport\":\"log_records_v1\"}}", self.valid, self.count).into_bytes()
        }))
    }
}

impl Drop for Sample<'_> {
    fn drop(&mut self) {
        self.histogram.record_ns(self.started.elapsed().as_nanos());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_transport_keeps_every_bin_within_the_native_log_limit() {
        let histogram = Histogram::new();
        for elapsed in 128..228 {
            histogram.record_ns(elapsed);
        }
        let snapshot = histogram.snapshot();
        let records: Vec<_> = snapshot.records(7).collect();
        assert_eq!(records.len(), 101);
        for record in &records {
            assert!(
                record.len() + b"DETAIL\t".len() <= 1023,
                "native logging would truncate a metric record"
            );
            let start = record.iter().position(|&b| b == b'{').unwrap();
            assert!(crate::json::parse(&record[start..], crate::json::TEST_LIMITS).is_some());
        }
        let summary = records.last().unwrap();
        let start = summary.iter().position(|&b| b == b'{').unwrap();
        let json = crate::json::parse(&summary[start..], crate::json::TEST_LIMITS).unwrap();
        assert_eq!(json.get(b"histogram_rows").unwrap().to_f64(), Some(100.0));
        assert_eq!(json.get(b"callbacks").unwrap().to_f64(), Some(100.0));
    }

    #[test]
    fn callback_transport_crc_matches_the_independent_controller_vector() {
        let histogram = Histogram::new();
        for _ in 0..99 {
            histogram.record_ns(1000);
        }
        histogram.record_ns(1_000_000);
        let snapshot = histogram.snapshot();
        let records: Vec<_> = snapshot.records(7).collect();
        let summary = records.last().unwrap();
        let start = summary.iter().position(|&b| b == b'{').unwrap();
        let json = crate::json::parse(&summary[start..], crate::json::TEST_LIMITS).unwrap();
        assert_eq!(
            json.get(b"histogram_crc32").unwrap().to_f64(),
            Some(1222455617.0)
        );
    }

    #[test]
    fn callback_histogram_bounds_cover_exact_and_large_samples() {
        for (sample, expected) in [
            (0, (0, 0)),
            (127, (127, 127)),
            (128, (128, 128)),
            (255, (255, 255)),
            (256, (256, 257)),
            (511, (510, 511)),
            (512, (512, 515)),
            (1000, (1000, 1003)),
            (1_000_000, (999_424, 1_003_519)),
            (u64::MAX, (18_374_686_479_671_623_680, u64::MAX)),
        ] {
            let histogram = Histogram::new();
            histogram.record_ns(u128::from(sample));
            let snapshot = histogram.snapshot();
            assert!(snapshot.valid);
            assert_eq!(snapshot.count, 1);
            assert_eq!(snapshot.p99_bounds_ns, Some(expected));
            assert!(expected.0 <= sample && sample <= expected.1);
        }
        assert!(std::mem::size_of::<Histogram>() <= 64 * 1024 + 128);
    }

    #[test]
    fn callback_p99_is_an_order_statistic_not_the_maximum() {
        let histogram = Histogram::new();
        for _ in 0..99 {
            histogram.record_ns(1000);
        }
        histogram.record_ns(1_000_000);
        let snapshot = histogram.snapshot();
        assert!(snapshot.valid);
        assert_eq!(snapshot.count, 100);
        assert_eq!(snapshot.p99_bounds_ns, Some((1000, 1003)));
        histogram.record_ns(1_000_000);
        let snapshot = histogram.snapshot();
        assert_eq!(snapshot.count, 101);
        assert_eq!(snapshot.p99_bounds_ns, Some((999_424, 1_003_519)));
    }

    #[test]
    fn callback_empty_and_overflow_are_not_zero_latency_success() {
        let histogram = Histogram::new();
        let empty = histogram.snapshot();
        assert_eq!(empty.count, 0);
        assert!(!empty.valid);
        assert_eq!(empty.p99_bounds_ns, None);
        histogram.record_ns(u128::from(u64::MAX) + 1);
        let invalid = histogram.snapshot();
        assert!(!invalid.valid);
        assert_eq!(invalid.p99_bounds_ns, None);
        let histogram = Histogram::new();
        histogram
            .count
            .store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
        histogram.record_ns(1000);
        let invalid = histogram.snapshot();
        assert!(!invalid.valid);
        assert_eq!(invalid.p99_bounds_ns, None);
    }

    #[test]
    fn callback_parallel_samples_are_not_lost() {
        let histogram = Histogram::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let histogram = &histogram;
                scope.spawn(move || {
                    for _ in 0..500 {
                        histogram.record_ns(1000);
                    }
                });
            }
        });
        let snapshot = histogram.snapshot();
        assert!(snapshot.valid);
        assert_eq!(snapshot.count, 2000);
        assert_eq!(snapshot.p99_bounds_ns, Some((1000, 1003)));
    }

    #[test]
    fn callback_sample_records_real_elapsed_time_on_early_return() {
        fn early_return(histogram: &Histogram, reached_end: &mut bool) {
            let _sample = histogram.sample();
            if std::hint::black_box(true) {
                return;
            }
            *reached_end = true;
        }
        let histogram = Histogram::new();
        let mut reached_end = false;
        early_return(&histogram, &mut reached_end);
        assert!(!reached_end);
        let snapshot = histogram.snapshot();
        assert!(snapshot.valid);
        assert_eq!(snapshot.count, 1);
        assert!(snapshot.p99_bounds_ns.is_some());
    }
}
