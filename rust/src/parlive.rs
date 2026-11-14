//! Admission for download-time PAR2 state. Reservations cover owned component
//! ceilings; they are not a process-wide RSS measurement.

use par_rs::streaming::SessionOptions;
use std::sync::{Arc, Mutex};

const MIB: u64 = 1024 * 1024;
const JOB_CEILING: u64 = 128 * MIB;
const FIXED_METADATA: u64 = 8 * MIB;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SessionGeneration(pub u64);

pub(crate) struct LiveSessionResources {
    pub options: SessionOptions,
    pub reservation_bytes: u64,
}

pub(crate) fn resources(
    slice_size: u64,
    effective_pool_bytes: u64,
) -> Option<LiveSessionResources> {
    if slice_size == 0 || !slice_size.is_multiple_of(4) {
        return None;
    }
    // Completion facts have their own shared-pool lease, with a one-MiB job cap.
    let budget = effective_pool_bytes.min(JOB_CEILING).checked_sub(MIB)?;
    // Independent staging, fold payload, scratch, fixed tracker metadata,
    // dynamic range metadata, and queued/active caller payload allowances. The extra MiB includes parser resync space.
    let working = slice_size.checked_mul(2)?.checked_add(MIB)?.max(4 * MIB);
    let solve = 16 * MIB;
    let fixed = working
        .checked_mul(6)?
        .checked_add(2 * solve)?
        .checked_add(FIXED_METADATA)?;
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
            ..SessionOptions::default()
        },
        reservation_bytes,
    })
}

#[derive(Default)]
pub(crate) struct BudgetPool {
    reserved: Mutex<u64>,
}

pub(crate) struct SessionBudgetLease {
    pool: Arc<BudgetPool>,
    bytes: u64,
}

impl BudgetPool {
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
        assert!(r.reservation_bytes + MIB <= 128 * MIB,
            "the completion context and session must share the per-job ceiling");
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
