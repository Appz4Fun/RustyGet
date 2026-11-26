//! Unpack slots: how many extractions (direct unpack, live extraction, post-processing unpack)
//! run at once, and which goes first.
//!
//! One global unpack lock used to serialize every extraction. On a busy queue (6790: about
//! 1000 groups) a paused or slow download's direct unpack held it while it waited for volumes
//! that would not come, so a new download's live extraction never started, a weak primary
//! could not be kept for its backup (the keep rule needs a running extraction) and finished
//! jobs waited in PP_QUEUED. Now:
//!
//! - up to [`capacity`] extractions run at once (option `UnpackSlots`; 0 picks
//!   `min(4, logical CPUs / 2)`, at least one);
//! - an extraction holds its slot only while it extracts: one waiting for a volume gives it
//!   back (`yield` in the direct unpack's wait callbacks) and asks again when the volume is
//!   there, so a paused download's extraction never keeps a slot from anyone;
//! - a waiter for a slot is admitted in [`Class`] order (an active download first, then a
//!   finished job's unpack, then a paused or held download), oldest first within a class.
//!   The class is asked again while waiting, so a download resumed meanwhile moves up.

use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// Who waits for a slot, in the order slots are handed out (a greater class goes first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// A download that is paused (whole, or every download is), or held by its fleet: it may
    /// extract what has arrived when no one else wants the slot.
    Paused = 0,
    /// A finished download's post-processing unpack.
    Finished = 1,
    /// A download that downloads now (a fleet's primary among them): its live extraction keeps
    /// up with the line, and a weak primary is kept for its backup only while it extracts.
    Active = 2,
}

/// What kind of extraction holds or waits for a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Direct unpack (a RAR set read while it downloads, or a live 7z/ZIP set).
    Direct,
    /// Post-processing unpack of a finished download.
    Post,
}

struct Waiter {
    ticket: u64,
    class: Class,
    kind: Kind,
}

struct Holder {
    ticket: u64,
    kind: Kind,
}

struct State {
    next_ticket: u64,
    holders: Vec<Holder>,
    waiters: Vec<Waiter>,
}

static STATE: Mutex<State> = Mutex::new(State {
    next_ticket: 1,
    holders: Vec::new(),
    waiters: Vec::new(),
});
static CHANGED: Condvar = Condvar::new();

/// How often a waiter looks at its class and whether it was stopped.
const RECHECK: Duration = Duration::from_millis(250);

fn lock() -> MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Slots for option `UnpackSlots` on a computer with `cores` logical CPUs: 0 (the default)
/// picks half the CPUs, at least one and at most four (each extraction may use several
/// threads of its own, see `FeedThreads`); n is n (at most 64).
pub fn capacity_for(option: i32, cores: usize) -> usize {
    if option > 0 {
        (option as usize).min(64)
    } else {
        (cores / 2).clamp(1, 4)
    }
}

/// Slots now, from the active options (read each time, so a reload applies).
pub fn capacity() -> usize {
    let option = crate::options::active_options().map_or(0, |o| o.scalars().unpack_slots);
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    capacity_for(option, cores)
}

/// The class of a download's direct unpack: [`Class::Paused`] when its remaining files cannot
/// arrive for now (`remaining_files` left and every one paused, or `downloads_paused`: every
/// download paused or the quota reached, and it is not forced) or its fleet holds it (`held`),
/// else [`Class::Active`].
pub fn download_class(remaining_files: usize, paused_files: i32, downloads_paused: bool, held: bool) -> Class {
    let all_paused = remaining_files > 0 && remaining_files as i64 <= i64::from(paused_files);
    if downloads_paused || all_paused || held {
        Class::Paused
    } else {
        Class::Active
    }
}

/// Whether the waiter `me` (class, ticket) gets one of `free` slots: fewer than `free`
/// waiters in `waiters` (`me` among them or not) go before it, a greater class first and an
/// older ticket first within a class.
pub fn admitted(free: usize, waiters: &[(Class, u64)], me: (Class, u64)) -> bool {
    if free == 0 {
        return false;
    }
    let ahead = waiters
        .iter()
        .filter(|&&(class, ticket)| {
            ticket != me.1 && (class > me.0 || (class == me.0 && ticket < me.1))
        })
        .count();
    ahead < free
}

/// Whether a finished job's unpack (class [`Class::Finished`]) finds a slot now: slots held,
/// waiters that go before it, and `starting` post jobs already in their unpack stage but not
/// yet holding or waiting for a slot leave one free.
pub fn room_for_finished(starting: usize) -> bool {
    room_for_finished_in(capacity(), &lock(), starting)
}

fn room_for_finished_in(cap: usize, st: &State, starting: usize) -> bool {
    let ahead = st
        .waiters
        .iter()
        .filter(|w| w.class >= Class::Finished)
        .count();
    st.holders.len() + ahead + starting < cap
}

/// Number of post jobs holding or waiting for a slot (to tell which jobs in their unpack
/// stage the pool already counts).
pub fn post_users() -> usize {
    let st = lock();
    st.holders.iter().filter(|h| h.kind == Kind::Post).count()
        + st.waiters.iter().filter(|w| w.kind == Kind::Post).count()
}

/// Slots held now.
pub fn in_use() -> usize {
    lock().holders.len()
}

/// A held slot; dropping it hands it to the next waiter.
#[must_use]
pub struct Slot {
    ticket: u64,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut st = lock();
        st.holders.retain(|h| h.ticket != self.ticket);
        drop(st);
        CHANGED.notify_all();
    }
}

/// Takes a slot for an extraction, waiting while none is free for it. `class` is asked
/// before the first wait and again every quarter second while waiting (it may lock the queue:
/// it is never called with the pool's lock held). `stopped` is asked as often: true gives up
/// (None). `on_wait(in_use, capacity)` is called once, when the first look found no slot.
pub fn acquire(
    kind: Kind,
    class: &mut dyn FnMut() -> Class,
    stopped: &mut dyn FnMut() -> bool,
    on_wait: &mut dyn FnMut(usize, usize),
) -> Option<Slot> {
    // Fast path: a free slot and no one waiting.
    let cap = capacity();
    let ticket = {
        let mut st = lock();
        let ticket = st.next_ticket;
        st.next_ticket += 1;
        if st.waiters.is_empty() && st.holders.len() < cap {
            st.holders.push(Holder {
                ticket,
                kind,
            });
            return Some(Slot { ticket });
        }
        ticket
    };
    let mut told = false;
    loop {
        if stopped() {
            let mut st = lock();
            st.waiters.retain(|w| w.ticket != ticket);
            drop(st);
            CHANGED.notify_all();
            return None;
        }
        let my_class = class();
        let cap = capacity();
        let mut st = lock();
        match st.waiters.iter_mut().find(|w| w.ticket == ticket) {
            Some(w) => w.class = my_class,
            None => st.waiters.push(Waiter {
                ticket,
                class: my_class,
                kind,
            }),
        }
        let free = cap.saturating_sub(st.holders.len());
        let order: Vec<(Class, u64)> = st.waiters.iter().map(|w| (w.class, w.ticket)).collect();
        if admitted(free, &order, (my_class, ticket)) {
            st.waiters.retain(|w| w.ticket != ticket);
            st.holders.push(Holder {
                ticket,
                kind,
            });
            return Some(Slot { ticket });
        }
        if !told {
            told = true;
            let in_use = st.holders.len();
            drop(st);
            on_wait(in_use, cap);
            continue;
        }
        let _ = CHANGED
            .wait_timeout(st, RECHECK)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_is_half_the_cpus_between_one_and_four_unless_set() {
        assert_eq!(capacity_for(0, 1), 1);
        assert_eq!(capacity_for(0, 2), 1);
        assert_eq!(capacity_for(0, 6), 3);
        assert_eq!(capacity_for(0, 12), 4);
        assert_eq!(capacity_for(0, 64), 4);
        assert_eq!(capacity_for(-1, 12), 4);
        assert_eq!(capacity_for(1, 12), 1);
        assert_eq!(capacity_for(8, 2), 8);
        assert_eq!(capacity_for(1000, 2), 64);
    }

    #[test]
    fn an_active_download_goes_before_a_finished_job_and_both_before_a_paused_one() {
        use Class::*;
        // one free slot; a paused download asked first, a finished job next, an active
        // download last: the active download gets it
        let waiters = [(Paused, 1), (Finished, 2), (Active, 3)];
        assert!(admitted(1, &waiters, (Active, 3)));
        assert!(!admitted(1, &waiters, (Finished, 2)));
        assert!(!admitted(1, &waiters, (Paused, 1)));
        // two free: the finished job too, still not the paused one
        assert!(admitted(2, &waiters, (Finished, 2)));
        assert!(!admitted(2, &waiters, (Paused, 1)));
        // a paused download alone takes a free slot (it extracts what has arrived)
        assert!(admitted(1, &[(Paused, 1)], (Paused, 1)));
        // no free slot: no one
        assert!(!admitted(0, &waiters, (Active, 3)));
    }

    #[test]
    fn within_a_class_the_oldest_waiter_goes_first() {
        use Class::*;
        let waiters = [(Active, 7), (Active, 4), (Active, 9)];
        assert!(admitted(1, &waiters, (Active, 4)));
        assert!(!admitted(1, &waiters, (Active, 7)));
        assert!(admitted(2, &waiters, (Active, 7)));
        assert!(!admitted(2, &waiters, (Active, 9)));
        // a waiter not listed yet counts the listed ones ahead of it
        assert!(!admitted(1, &[(Active, 1)], (Active, 2)));
        assert!(admitted(1, &[(Paused, 1)], (Active, 2)));
    }

    #[test]
    fn a_resumed_download_moves_ahead_of_older_finished_jobs() {
        use Class::*;
        // ticket 1 waited as paused; resumed, it is active and goes first
        assert!(!admitted(1, &[(Paused, 1), (Finished, 2)], (Paused, 1)));
        assert!(admitted(1, &[(Active, 1), (Finished, 2)], (Active, 1)));
    }

    #[test]
    fn a_paused_or_held_download_waits_behind_the_others() {
        use Class::*;
        // paused whole
        assert_eq!(download_class(5, 5, false, false), Paused);
        // a paused par2 among files still downloading is no pause
        assert_eq!(download_class(5, 1, false, false), Active);
        assert_eq!(download_class(5, 0, false, false), Active);
        // every file arrived: it finishes what it has, as an active download
        assert_eq!(download_class(0, 0, false, false), Active);
        // every download paused (or the quota reached), and it is not forced
        assert_eq!(download_class(5, 0, true, false), Paused);
        // held by its fleet
        assert_eq!(download_class(5, 0, false, true), Paused);
    }

    fn state(holders: usize, waiters: &[Class]) -> State {
        State {
            next_ticket: 100,
            holders: (0..holders)
                .map(|i| Holder {
                    ticket: i as u64,
                    kind: Kind::Direct,
                })
                .collect(),
            waiters: waiters
                .iter()
                .enumerate()
                .map(|(i, &class)| Waiter {
                    ticket: 50 + i as u64,
                    class,
                    kind: Kind::Direct,
                })
                .collect(),
        }
    }

    #[test]
    fn a_finished_job_starts_unpacking_only_with_a_slot_left_for_it() {
        use Class::*;
        assert!(room_for_finished_in(2, &state(0, &[]), 0));
        assert!(room_for_finished_in(2, &state(1, &[]), 0));
        assert!(!room_for_finished_in(2, &state(2, &[]), 0));
        // a paused download waiting for a slot holds no finished job back
        assert!(room_for_finished_in(2, &state(1, &[Paused, Paused]), 0));
        // an active download waiting goes first
        assert!(!room_for_finished_in(2, &state(1, &[Active]), 0));
        // a job already entering its unpack counts
        assert!(!room_for_finished_in(2, &state(1, &[]), 1));
    }
}
