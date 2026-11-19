//! Admission for download-time PAR2 state. Reservations cover owned component
//! ceilings; they are not a process-wide RSS measurement.

use par_rs::repairer::RepairError;
use par_rs::streaming::SessionOptions;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

const MIB: u64 = 1024 * 1024;
pub(crate) const JOB_CEILING: u64 = 128 * MIB;
/// Syndrome slots a job with a large slice is allowed to grow its ceiling for.
const MIN_SLOTS: u64 = 2;
const FIXED_METADATA: u64 = 8 * MIB;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SessionGeneration(pub u64);

pub(crate) struct LiveSessionResources {
    pub options: SessionOptions,
    pub reservation_bytes: u64,
}

/// What a live session for `slice_size` needs at the least (MIN_SLOTS
/// syndrome slots), for the message when it does not fit.
pub(crate) fn min_reservation(slice_size: u64) -> u64 {
    let working = slice_size.saturating_mul(2).saturating_add(MIB).max(4 * MIB);
    working
        .saturating_mul(6)
        .saturating_add(32 * MIB)
        .saturating_add(FIXED_METADATA)
        .saturating_add(slice_size.saturating_mul(2 * MIN_SLOTS))
        .saturating_add(MIB)
}

pub(crate) fn resources(
    slice_size: u64,
    effective_pool_bytes: u64,
) -> Option<LiveSessionResources> {
    if slice_size == 0 || !slice_size.is_multiple_of(4) {
        return None;
    }
    // Independent staging, fold payload, scratch, fixed tracker metadata,
    // dynamic range metadata, and queued/active caller payload allowances. The extra MiB includes parser resync space.
    let working = slice_size.checked_mul(2)?.checked_add(MIB)?.max(4 * MIB);
    let solve = 16 * MIB;
    let fixed = working
        .checked_mul(6)?
        .checked_add(2 * solve)?
        .checked_add(FIXED_METADATA)?;
    // A job is held to JOB_CEILING, except that a large slice may take what
    // MIN_SLOTS syndrome slots need (a 2160p release often has 6-10 MiB
    // slices, which never fit 128 MiB); the pool still bounds it.
    let needed = fixed
        .checked_add(slice_size.checked_mul(2 * MIN_SLOTS)?)?
        .checked_add(MIB)?;
    // Completion facts have their own shared-pool lease, with a one-MiB job cap.
    let budget = effective_pool_bytes
        .min(JOB_CEILING.max(needed))
        .checked_sub(MIB)?;
    let syndrome = budget.checked_sub(fixed)? / 2;
    let slots = (syndrome / slice_size).min(512) as usize;
    if slots == 0 {
        return None;
    }
    let syndrome = slice_size.checked_mul(slots as u64)?;
    let reservation_bytes = fixed.checked_add(2 * syndrome)?;
    Some(LiveSessionResources {
        options: SessionOptions {
            preallocated_syndrome_slots: slots.min(4),
            max_syndrome_slots: slots,
            max_syndrome_bytes: syndrome,
            max_working_bytes: working,
            max_staging_bytes: working,
            solve_mem_limit_bytes: solve,
            solve_threads: 1,
            fold_threads: 0,
            // Re-reading verified slices for new slots runs on its own thread
            // (`streaming_session_kick_backfill`), never under the queue lock.
            defer_backfill: true,
            ..SessionOptions::default()
        },
        reservation_bytes,
    })
}

#[derive(Default)]
pub(crate) struct BudgetPool {
    reserved: Mutex<u64>,
    /// A fixed size instead of the effective ParBuffer (tests' own pools).
    size: Option<u64>,
}

pub(crate) struct SessionBudgetLease {
    pool: Arc<BudgetPool>,
    bytes: u64,
}

impl BudgetPool {
    /// A pool of its own with a fixed size, apart from the daemon's shared one.
    #[cfg(test)]
    pub fn with_size(bytes: u64) -> Self {
        Self {
            reserved: Mutex::new(0),
            size: Some(bytes),
        }
    }

    #[cfg(test)]
    pub fn reserved_for_test(&self) -> std::sync::MutexGuard<'_, u64> {
        self.reserved.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The fixed size, if this pool has one.
    pub fn size(&self) -> Option<u64> {
        self.size
    }

    pub fn acquire(self: &Arc<Self>, bytes: u64, limit: u64) -> Option<SessionBudgetLease> {
        if bytes == 0 {
            return None;
        }
        let mut used = self.reserved.lock().unwrap_or_else(|e| e.into_inner());
        let next = used.checked_add(bytes)?;
        if next > limit {
            return None;
        }
        *used = next;
        Some(SessionBudgetLease {
            pool: Arc::clone(self),
            bytes,
        })
    }
}

impl Drop for SessionBudgetLease {
    fn drop(&mut self) {
        let mut used = self.pool.reserved.lock().unwrap_or_else(|e| e.into_inner());
        *used -= self.bytes;
    }
}

impl SessionBudgetLease {
    /// The pool this lease draws from.
    pub fn pool(&self) -> &Arc<BudgetPool> {
        &self.pool
    }

    #[cfg(test)]
    pub fn reserved_bytes(&self) -> u64 {
        self.bytes
    }

    /// Grow by as many `unit`-byte steps as the pool has room for, up to
    /// `max_units`. Returns the steps granted (0 when the pool is full).
    pub fn grow_by_units(&mut self, unit: u64, max_units: u64, limit: u64) -> u64 {
        if unit == 0 {
            return 0;
        }
        let mut used = self.pool.reserved.lock().unwrap_or_else(|e| e.into_inner());
        let units = (limit.saturating_sub(*used) / unit).min(max_units);
        *used += units * unit;
        self.bytes += units * unit;
        units
    }

    /// Take over another lease of the same pool.
    pub fn absorb(&mut self, mut other: SessionBudgetLease) {
        self.bytes += other.bytes;
        other.bytes = 0;
    }

    /// Give `bytes` back to the pool (never below one byte, so the lease stays valid).
    pub fn shrink_by(&mut self, bytes: u64) {
        let bytes = bytes.min(self.bytes.saturating_sub(1));
        let mut used = self.pool.reserved.lock().unwrap_or_else(|e| e.into_inner());
        *used -= bytes;
        self.bytes -= bytes;
    }

    pub fn resize(&mut self, bytes: u64, limit: u64) -> bool {
        if bytes == 0 {
            return false;
        }
        let mut used = self.pool.reserved.lock().unwrap_or_else(|e| e.into_inner());
        let Some(next) = (*used - self.bytes).checked_add(bytes) else {
            return false;
        };
        if next > limit {
            return false;
        }
        *used = next;
        self.bytes = bytes;
        true
    }
}

pub(crate) static LIVE_BUDGET: LazyLock<Arc<BudgetPool>> =
    LazyLock::new(|| Arc::new(BudgetPool::default()));

#[derive(Default)]
struct ControlAccounting {
    bytes: u64,
    jobs: HashMap<i32, u64>,
}
static CONTROL_ACCOUNTING: LazyLock<Mutex<ControlAccounting>> =
    LazyLock::new(|| Mutex::new(ControlAccounting::default()));

pub(crate) struct ControlLease {
    nzb_id: i32,
    bytes: u64,
    pool_limit: u64,
    pool_optional: bool,
    // None only for ordinary-write root control: still capped by the control
    // queue, but never blocked by an exhausted live PAR pool.
    _budget: Option<SessionBudgetLease>,
}
impl ControlLease {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Charge at least `extra` more bytes under the caps of the original
    /// reservation (rounded up to 1 KiB). On refusal nothing changes.
    pub fn grow(&mut self, extra: u64) -> Result<(), RepairError> {
        let extra = extra.max(1).div_ceil(1024).saturating_mul(1024);
        let mut more = reserve_control_inner(self.nzb_id, extra, self.pool_limit, self.pool_optional)?;
        match (&mut self._budget, more._budget.take()) {
            (Some(mine), Some(theirs)) => mine.absorb(theirs),
            (slot @ None, theirs) => *slot = theirs,
            (Some(_), None) => {}
        }
        self.bytes += more.bytes;
        // The merged bytes now belong to self; `more` must not release them.
        more.bytes = 0;
        Ok(())
    }
}
impl Drop for ControlLease {
    fn drop(&mut self) {
        let mut used = CONTROL_ACCOUNTING.lock().unwrap_or_else(|e| e.into_inner());
        used.bytes -= self.bytes;
        if let Some(job) = used.jobs.get_mut(&self.nzb_id) {
            *job -= self.bytes;
            if *job == 0 {
                used.jobs.remove(&self.nzb_id);
            }
        }
    }
}

pub(crate) fn reserve_control(
    nzb_id: i32,
    bytes: u64,
    pool_limit: u64,
) -> Result<ControlLease, RepairError> {
    reserve_control_inner(nzb_id, bytes, pool_limit, false)
}

/// Charge physical-root control for an ordinary source write. The shared live
/// PAR pool is preferred, but its exhaustion must not fail plain downloads.
pub(crate) fn reserve_root_control(
    nzb_id: i32,
    bytes: u64,
    pool_limit: u64,
) -> Result<ControlLease, RepairError> {
    reserve_control_inner(nzb_id, bytes, pool_limit, true)
}

fn reserve_control_inner(
    nzb_id: i32,
    bytes: u64,
    pool_limit: u64,
    pool_optional: bool,
) -> Result<ControlLease, RepairError> {
    let limit_error = |context, required, limit| RepairError::StreamingMemoryLimit {
        context,
        required_bytes: required,
        limit_bytes: limit,
    };
    if !(256..=MIB).contains(&bytes) {
        return Err(limit_error("live control context", bytes, MIB));
    }
    let mut used = CONTROL_ACCOUNTING.lock().unwrap_or_else(|e| e.into_inner());
    let total = used.bytes.checked_add(bytes);
    let job = used
        .jobs
        .get(&nzb_id)
        .copied()
        .unwrap_or(0)
        .checked_add(bytes);
    // Ordinary-write root control is accounted (so live reservations yield to
    // it) but never refused by the live caps.
    if pool_optional {
        let budget = LIVE_BUDGET.acquire(bytes, pool_limit);
        used.bytes = used.bytes.saturating_add(bytes);
        *used.jobs.entry(nzb_id).or_insert(0) += bytes;
        return Ok(ControlLease {
            nzb_id,
            bytes,
            pool_limit,
            pool_optional,
            _budget: budget,
        });
    }
    if total.is_none_or(|n| n > 8 * MIB) {
        return Err(limit_error(
            "live control queue",
            total.unwrap_or(u64::MAX),
            8 * MIB,
        ));
    }
    if job.is_none_or(|n| n > MIB) {
        return Err(limit_error(
            "live job control context",
            job.unwrap_or(u64::MAX),
            MIB,
        ));
    }
    let budget = LIVE_BUDGET.acquire(bytes, pool_limit);
    if budget.is_none() {
        return Err(limit_error("shared live PAR reservation", bytes, pool_limit));
    }
    used.bytes = total.unwrap();
    used.jobs.insert(nzb_id, job.unwrap());
    Ok(ControlLease {
        nzb_id,
        bytes,
        pool_limit,
        pool_optional,
        _budget: budget,
    })
}

mod write_control;
pub(crate) use write_control::{
    RepairWriteLease, WriteBlocked, WriteCoordinator, WriterIdentity, WriterTarget,
};

#[cfg(all(test, unix))]
mod physical_ownership_tests;
mod source_identity;
mod write_registry;
pub(crate) use write_registry::{
    donor_write_context, download_writes, forget_download_writes, open_repair_target,
    repair_target_is_current, source_write_context, SourceWriteContext,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn four_slots_fit_the_common_100gb_slice_with_bounded_reservation() {
        let r = resources(4 * MIB, 500 * MIB).unwrap();
        assert_eq!(r.options.preallocated_syndrome_slots, 4);
        assert!(r.options.max_syndrome_slots >= 4);
        assert!(r.reservation_bytes <= 128 * MIB);
        assert!(
            2 * r.options.max_syndrome_bytes
                + 6 * r.options.max_working_bytes
                + 2 * r.options.solve_mem_limit_bytes
                <= r.reservation_bytes
        );
    }

    #[test]
    fn large_slice_reduces_slots_and_insufficient_budget_falls_back() {
        let r = resources(5 * MIB, 128 * MIB).unwrap();
        assert_eq!(r.options.preallocated_syndrome_slots, 2);
        assert!(resources(6 * MIB, 128 * MIB).is_none());
        // a 2160p release's 6.15 MiB slice (live 2026-11-15) fits a 500 MiB
        // pool with two slots, just above the 128 MiB job ceiling
        let r = resources(6_451_200, 500 * MIB).unwrap();
        assert_eq!(r.options.max_syndrome_slots, 2);
        assert!(r.reservation_bytes <= 160 * MIB, "{}", r.reservation_bytes);
        assert!(resources(10 * MIB, 500 * MIB).unwrap().reservation_bytes <= 220 * MIB);
        // the message names what a slice really needs
        assert!(min_reservation(6_451_200) > 128 * MIB);
        assert!(resources(6_451_200, min_reservation(6_451_200)).is_some());
        assert!(resources(4 * MIB, 16 * MIB).is_none());
        assert!(resources(0, 500 * MIB).is_none());
        assert!(resources(u64::MAX, 500 * MIB).is_none());
    }

    #[test]
    fn concurrent_jobs_share_budget_and_release_only_after_last_owner() {
        let pool = Arc::new(BudgetPool::default());
        let a = Arc::new(pool.acquire(80, 100).unwrap());
        let active_operation = Arc::clone(&a);
        assert!(pool.acquire(30, 100).is_none());
        drop(a);
        assert!(pool.acquire(30, 100).is_none());
        drop(active_operation);
        let b = pool.acquire(100, 100).unwrap();
        assert!(pool.acquire(1, 100).is_none());
        drop(b);
        assert!(pool.acquire(1, 100).is_some());
    }

    #[test]
    fn growth_takes_only_free_whole_steps_and_gives_them_back() {
        let pool = Arc::new(BudgetPool::default());
        let mut job = pool.acquire(40, 100).unwrap();
        let other = pool.acquire(25, 100).unwrap();
        // 35 free: three 10-byte steps of the five asked for.
        assert_eq!(job.grow_by_units(10, 5, 100), 3);
        assert!(pool.acquire(6, 100).is_none());
        assert_eq!(job.grow_by_units(10, 5, 100), 0);
        assert_eq!(job.grow_by_units(0, 5, 100), 0);
        // A limit under the current use (memory pressure) grants nothing.
        assert_eq!(job.grow_by_units(1, 5, 50), 0);
        job.shrink_by(20);
        assert!(pool.acquire(25, 100).is_some());
        drop(other);
        job.shrink_by(u64::MAX);
        assert_eq!(job.bytes, 1);
        drop(job);
        assert!(pool.acquire(100, 100).is_some());
    }

    #[test]
    fn pressure_rejects_new_admission_without_forgiving_existing_charge() {
        let pool = Arc::new(BudgetPool::default());
        let a = pool.acquire(80, 100).unwrap();
        assert!(pool.acquire(1, 50).is_none());
        drop(a);
        assert!(pool.acquire(50, 50).is_some());
    }

    #[test]
    fn aggregate_admission_rejects_overflow_and_zero_reservations() {
        let pool = Arc::new(BudgetPool::default());
        assert!(pool.acquire(0, 100).is_none());
        let a = pool.acquire(u64::MAX, u64::MAX).unwrap();
        assert!(pool.acquire(1, u64::MAX).is_none());
        drop(a);
        assert!(pool.acquire(1, 1).is_some());
    }

    #[test]
    fn metadata_reservation_grows_atomically_without_forgiving_existing_owners() {
        let pool = Arc::new(BudgetPool::default());
        let mut loading = pool.acquire(8, 100).unwrap();
        let other = pool.acquire(30, 100).unwrap();
        assert!(!loading.resize(80, 100));
        assert!(pool.acquire(63, 100).is_none());
        drop(other);
        assert!(loading.resize(80, 100));
        assert!(pool.acquire(21, 100).is_none());
        drop(loading);
        assert!(pool.acquire(100, 100).is_some());
    }

    #[test]
    fn completion_context_has_headroom_at_the_syndrome_boundary() {
        let r = resources(64 * 1024, 500 * MIB).unwrap();
        assert!(
            r.reservation_bytes + MIB <= 128 * MIB,
            "the completion context and session must share the per-job ceiling"
        );
    }

    #[test]
    fn first_recovery_file_is_absorbed_once_when_it_starts_the_session() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("rustyget-first-par-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/testdata/parchecker");
        for entry in std::fs::read_dir(fixtures).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), dir.join(entry.file_name())).unwrap();
        }
        let id = -1900001;
        let first = dir.join("testfile.vol00+1.PAR2");
        crate::parchecker::streaming_session_start_bounded(id, &first, &dir, 500 * MIB).unwrap();
        let ready = crate::parchecker::streaming_session_status(id)
            .unwrap()
            .ready_recovery;
        let replay =
            crate::parchecker::streaming_session_on_recovery_volume_arrived(id, &first, None)
                .unwrap()
                .unwrap();
        crate::parchecker::streaming_session_forget(id);
        std::fs::remove_dir_all(dir).unwrap();
        assert!(
            ready >= 1,
            "the triggering recovery file supplied no ready recovery"
        );
        assert_eq!(
            replay, 0,
            "replaying completion must not add recovery twice"
        );
    }
}

#[cfg(test)]
mod write_tests;
