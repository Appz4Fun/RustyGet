//! Measurements from the owned caller boundary, separate from RAM-only engine input.
use super::*;
use std::time::{Duration, Instant};

pub(super) fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[derive(Default)]
pub(super) struct LiveMetricCounters {
    pub captured_articles: AtomicU64,
    pub captured_bytes: AtomicU64,
    pub capture_refusals: AtomicU64,
    pub cache_misses: AtomicU64,
    /// Cache misses handed to par-rs without bytes (read from their written file).
    pub committed_articles: AtomicU64,
    /// Articles RAM could not take that waited for the cache to write them out.
    pub awaited_commits: AtomicU64,
    /// Cache misses taken from bytes kept aside when the article was written (no read-back).
    pub stashed_articles: AtomicU64,
    /// Articles written before their capture whose bytes could not be kept (no room even
    /// after a loan of free ParBuffer): par-rs reads them back from their file.
    pub stash_refusals: AtomicU64,
    /// Cache misses taken from bytes kept when the article was written before the live
    /// session started (`presession`), not read back.
    pub presession_articles: AtomicU64,
    /// Repeated captures of an article already queued with its bytes from RAM (same size
    /// and CRC32): skipped, not read back from its file.
    pub repeated_captures: AtomicU64,
    /// Articles that waited for the cache write, handed over with the bytes it wrote.
    pub written_handoffs: AtomicU64,
    /// `articledl::SNAPSHOTS_DURING_FLUSH` when the session started: the I/O line reports
    /// the articles copied from the cache during a flush of their file since then.
    pub flush_snapshots_base: AtomicU64,
    /// Obfuscated queue files bound to their par2 file by their first article's 16k hash.
    pub bound_by_first_article: AtomicU64,
    /// Verified slices folded into recovery slots added later from bytes still in RAM
    /// (`backfill_from_bytes`), not re-read from disk.
    pub ram_backfilled_slices: AtomicU64,
    /// Recovery volumes absorbed from the bytes RustyGet wrote (`on_recovery_volume_bytes`),
    /// and their bytes: never read back.
    pub recovery_volumes_from_ram: AtomicU64,
    pub recovery_bytes_from_ram: AtomicU64,
    /// Damaged slices decode-ahead rebuilt while the download went on (par-rs
    /// `take_rebuilt_slices`), and those re-verified once their file completed.
    pub rebuilt_ahead_slices: AtomicU64,
    pub rebuilt_rechecked: AtomicU64,
    pub writer_wait_ns: AtomicU64,
    pub operation_wait_ns: AtomicU64,
    report_sequence: AtomicU64,
    report_nzb_id: std::sync::atomic::AtomicI32,
    count_ready_at: Mutex<Option<Instant>>,
    count_ready_to_handoff_ns: Mutex<Option<u64>>,
}

pub(crate) struct LiveCallerMetrics {
    pub generation: SessionGeneration,
    pub captured_articles: u64,
    pub captured_bytes: u64,
    pub capture_refusals: u64,
    pub cache_misses: u64,
    pub committed_replay_bytes: u64,
    pub io: LiveIoStats,
    pub repair: par_rs::streaming::RepairPhaseStats,
    pub count_ready_at: Option<Instant>,
    pub count_ready_to_handoff_ns: Option<u64>,
    pub writer_wait_ns: u64,
    pub operation_wait_ns: u64,
    pub status: LiveStatus,
}

impl ActiveStreamingSession {
    pub(super) fn observe_count_readiness(&self) {
        if self.session.status().early_cutover_possible {
            let mut ready = self
                .metrics
                .count_ready_at
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            ready.get_or_insert_with(Instant::now);
        }
    }
    pub(super) fn record_live_handoff(&self) {
        let ready = *self
            .metrics
            .count_ready_at
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(ready) = ready {
            *self
                .metrics
                .count_ready_to_handoff_ns
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(nanos(ready.elapsed()));
        }
    }
    fn live_metrics(&self) -> LiveCallerMetrics {
        LiveCallerMetrics {
            generation: self.generation,
            captured_articles: self.metrics.captured_articles.load(Ordering::Relaxed),
            captured_bytes: self.metrics.captured_bytes.load(Ordering::Relaxed),
            capture_refusals: self.metrics.capture_refusals.load(Ordering::Relaxed),
            cache_misses: self.metrics.cache_misses.load(Ordering::Relaxed),
            committed_replay_bytes: self.catchup_source_bytes.load(Ordering::Relaxed),
            io: self.session.io_stats(),
            repair: self.session.repair_phase_stats(),
            count_ready_at: *self
                .metrics
                .count_ready_at
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            count_ready_to_handoff_ns: *self
                .metrics
                .count_ready_to_handoff_ns
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            writer_wait_ns: self.metrics.writer_wait_ns.load(Ordering::Relaxed),
            operation_wait_ns: self.metrics.operation_wait_ns.load(Ordering::Relaxed),
            status: self.session.status(),
        }
    }
    pub(super) fn emit_live_metrics(&self, nzb_id: i32, event: &str) {
        self.metrics.report_nzb_id.store(nzb_id, Ordering::Relaxed);
        let Ok(previous) =
            self.metrics
                .report_sequence
                .try_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
        else {
            crate::log::log_active_bytes(
                crate::queuesubsys_host::MK_DETAIL,
                b"Live PAR frame {\"version\":1,\"valid\":false}",
            );
            return;
        };
        let snapshot = self.live_metrics();
        for frame in snapshot.log_frames(nzb_id, event, previous + 1) {
            crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, &frame);
        }
    }
}

impl ActiveStreamingSession {
    /// Per-job I/O of live repair (`io_stats`): source bytes read back from disk (par-rs's
    /// reads for articles RAM did not hold, plus RustyGet's completion replays), back-fill
    /// re-reads (0 when the recovery slots were planned up front), recovery bytes.
    pub(super) fn io_stats_line(&self, nzb_id: i32) -> String {
        let io = self.session.io_stats();
        let replay = self.catchup_source_bytes.load(Ordering::Relaxed);
        let mb = |bytes: u64| format!("{:.1}", bytes as f64 / (1024.0 * 1024.0));
        format!(
            "Live PAR (job {nzb_id}) I/O: source_bytes_read {} MB (par-rs {} MB, RustyGet replay {} MB), backfill_bytes_read {} MB, recovery_bytes_read {} MB, target_verification_bytes_read {} MB, ram_bytes_verified {} MB, ram_backfilled_slices {}, recovery volumes from RAM {} ({} MB), trusted slice checks {}; {} cache miss(es), {} article(s) read from their file by par-rs, {} waited for the cache flush, {} still waiting, {} handed over with the bytes written, {} taken from bytes kept at write, {} file(s) matched by their first article, {} not kept at write (no room), {} taken from bytes kept before the session, {} slice(s) rebuilt ahead ({} re-verified at completion), {} copied from the cache during a flush (all jobs), {} repeated capture(s) skipped (queued from RAM already)",
            mb(io.source_bytes_read.saturating_add(replay)),
            mb(io.source_bytes_read),
            mb(replay),
            mb(io.backfill_bytes_read),
            mb(io.recovery_bytes_read),
            mb(io.target_verification_bytes_read),
            mb(io.ram_bytes_verified),
            self.metrics.ram_backfilled_slices.load(Ordering::Relaxed),
            self.metrics.recovery_volumes_from_ram.load(Ordering::Relaxed),
            mb(self.metrics.recovery_bytes_from_ram.load(Ordering::Relaxed)),
            if self.trust_revoked.load(Ordering::Relaxed) { "off" } else { "on" },
            self.metrics.cache_misses.load(Ordering::Relaxed),
            self.metrics.committed_articles.load(Ordering::Relaxed),
            self.metrics.awaited_commits.load(Ordering::Relaxed),
            self.awaiting_commits(),
            self.metrics.written_handoffs.load(Ordering::Relaxed),
            self.metrics.stashed_articles.load(Ordering::Relaxed),
            self.metrics.bound_by_first_article.load(Ordering::Relaxed),
            self.metrics.stash_refusals.load(Ordering::Relaxed),
            self.metrics.presession_articles.load(Ordering::Relaxed),
            self.metrics.rebuilt_ahead_slices.load(Ordering::Relaxed),
            self.metrics.rebuilt_rechecked.load(Ordering::Relaxed),
            crate::articledl::SNAPSHOTS_DURING_FLUSH
                .load(Ordering::Relaxed)
                .saturating_sub(self.metrics.flush_snapshots_base.load(Ordering::Relaxed)),
            self.metrics.repeated_captures.load(Ordering::Relaxed),
        )
    }
}

impl LiveMetricCounters {
    pub(super) fn report_nzb_id(&self) -> i32 {
        self.report_nzb_id.load(Ordering::Relaxed)
    }
}

impl Drop for ActiveStreamingSession {
    fn drop(&mut self) {
        // No registry lookup (the registry lock may be held): match by generation.
        forget_live_poll(self.metrics.report_nzb_id(), self.generation);
        // No writer-gate lookup here: replacement can drop an entry under that
        // gate. Only installed/reported cohorts produce a disposal footer.
        if self.metrics.report_sequence.load(Ordering::Acquire) != 0 {
            let nzb_id = self.metrics.report_nzb_id.load(Ordering::Relaxed);
            // bytes read back from disk (par-rs reads, RustyGet replays, back-fill) go
            // into the job's repair record
            let io = self.session.io_stats();
            crate::repairsummary::note_read_back(
                nzb_id,
                io.source_bytes_read
                    .saturating_add(self.catchup_source_bytes.load(Ordering::Relaxed))
                    .saturating_add(io.backfill_bytes_read),
            );
            self.emit_live_metrics(nzb_id, "disposed");
            if let Some(summary) = self.refusal_summary() {
                let message = format!("Live PAR (job {nzb_id}) refusals: {summary}");
                crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, message.as_bytes());
            }
            let message = self.io_stats_line(nzb_id);
            crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, message.as_bytes());
        }
    }
}

impl LiveCallerMetrics {
    pub(super) fn log_frames(
        &self,
        nzb_id: i32,
        event: &str,
        report_id: u64,
    ) -> impl Iterator<Item = Vec<u8>> {
        const CHUNK: usize = 256;
        // Events are internal labels, never unbounded user input. Refuse a future
        // oversized label before allocating its serialized message.
        let payload = if event.len() <= 128 {
            self.log_line(nzb_id, event)
        } else {
            Vec::new()
        };
        let valid = !payload.is_empty() && payload.len() <= 64 * CHUNK;
        let parts = if valid {
            payload.len().div_ceil(CHUNK)
        } else {
            1
        };
        let checksum = crc32fast::hash(&payload);
        let generation = self.generation.0;
        (0..parts).map(move |index| {
            if !valid {
                return b"Live PAR frame {\"version\":1,\"valid\":false}".to_vec();
            }
            use std::io::Write;
            let mut line = Vec::with_capacity(768);
            let _ = write!(&mut line, "Live PAR frame {{\"version\":1,\"nzb_id\":{nzb_id},\"generation\":{generation},\"report_id\":{report_id},\"index\":{index},\"parts\":{parts},\"crc32\":{checksum},\"hex\":\"");
            for byte in &payload[index * CHUNK..payload.len().min((index + 1) * CHUNK)] {
                let _ = write!(&mut line, "{byte:02x}");
            }
            line.extend_from_slice(b"\"}");
            line
        })
    }
    pub(super) fn log_line(&self, nzb_id: i32, event: &str) -> Vec<u8> {
        use std::io::Write;
        let io = self.io;
        let repair = self.repair;
        let mut line = Vec::with_capacity(2048);
        line.extend_from_slice(b"Live PAR metrics {\"event\":\"");
        crate::escape::json_encode(event.as_bytes(), &mut line);
        let _ = write!(&mut line, "\",\"nzb_id\":{nzb_id}");
        for (key, value) in [
            ("generation", self.generation.0),
            ("captured_articles", self.captured_articles),
            ("captured_bytes", self.captured_bytes),
            ("capture_refusals", self.capture_refusals),
            ("cache_misses", self.cache_misses),
            ("committed_replay_bytes", self.committed_replay_bytes),
            ("ram_bytes_verified", io.ram_bytes_verified),
            ("source_bytes_read", io.source_bytes_read),
            ("backfill_bytes_read", io.backfill_bytes_read),
            ("recovery_bytes_read", io.recovery_bytes_read),
            (
                "target_verification_bytes_read",
                io.target_verification_bytes_read,
            ),
            ("tracked_exponents", self.status.syndrome_slots as u64),
            ("ready_recovery", self.status.ready_recovery as u64),
            ("pending_folds", self.status.pending_folds as u64),
            ("verified_slices", self.status.verified as u64),
            ("total_slices", self.status.total as u64),
            ("writer_wait_ns", self.writer_wait_ns),
            ("operation_wait_ns", self.operation_wait_ns),
            ("repair_attempts", repair.attempts),
            ("repair_completed_calls", repair.completed),
            ("target_bytes_written", repair.target_bytes_written),
            ("payload_wait_ns", repair.payload_wait_ns),
            ("slot_backfill_ns", repair.backfill_ns),
            ("matrix_ns", repair.matrix_ns),
            ("decode_ns", repair.decode_ns),
            ("target_write_ns", repair.target_write_ns),
            ("target_sync_ns", repair.target_sync_ns),
            ("refold_ns", repair.refold_ns),
            ("target_verification_ns", repair.target_verification_ns),
            ("repair_call_ns", repair.total_ns),
        ] {
            let _ = write!(&mut line, ",\"{key}\":{value}");
        }
        let handoff = self
            .count_ready_to_handoff_ns
            .map_or_else(|| "null".to_owned(), |value| value.to_string());
        let _ = write!(&mut line, ",\"count_ready_observed\":{},\"count_ready_to_handoff_ns\":{handoff},\"decode_includes_writes\":true,\"refold_includes_queued_completion\":false,\"logical_io\":true}}", self.count_ready_at.is_some());
        line
    }
}

#[cfg(test)]
pub(super) fn streaming_session_metrics(nzb_id: i32) -> Option<LiveCallerMetrics> {
    Some(active_streaming_session(nzb_id)?.live_metrics())
}
