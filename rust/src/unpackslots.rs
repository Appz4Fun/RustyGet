//! Unpack slots: how many extractions (direct unpack, live extraction, post-processing unpack)
//! run at once, and which goes first.
//!
//! One global unpack lock used to serialize every extraction. On a busy queue (6790: about
//! 1000 groups) a paused or slow download's direct unpack held it while it waited for volumes
//! that would not come, so a new download's live extraction never started, a weak primary
//! could not be kept for its backup (the keep rule needs a running extraction) and finished
//! jobs waited in PP_QUEUED. Now:
//!
//! - up to [`capacity`] extractions run at once (option `UnpackSlots`; 0, the default, is
//!   one: extractions share the destination disk, so one at a time finishes each release
//!   sooner);
//! - an extraction holds its slot only while it extracts: one waiting for a volume gives it
//!   back (`yield` in the direct unpack's wait callbacks) and asks again when the volume is
//!   there, so a paused download's extraction never keeps a slot from anyone;
//! - a waiter for a slot is admitted in [`Class`] order (an active download first, then a
//!   finished job's unpack, then a paused or held download), oldest first within a class.
//!   The class is asked again while waiting, so a download resumed meanwhile moves up;
//! - a finished job's unpack always enters the waiting line (it is never just skipped while
//!   slots are held, [`room_for_finished`] counts only other finished jobs' unpacks), so it
//!   ages like any waiter and cannot be passed over for ever;
//! - a direct unpack holding a slot gives it up at a safe point between volumes or members
//!   ([`should_yield`]) when a waiter of a greater class wants it, or a finished job's unpack
//!   waited [`OVERDUE`]. A finished job's unpack is never interrupted: it is the next release
//!   to finish.

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
    since: std::time::Instant,
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

/// Slots for option `UnpackSlots`: 0 (the default) is one, n is n (at most 64). Extractions
/// share one destination disk, so running several at once splits its bandwidth and every
/// release finishes later; one at a time finishes each release sooner for the same total.
/// One extraction still uses several threads of its own (see `FeedThreads`).
pub fn capacity_for(option: i32) -> usize {
    if option > 0 {
        (option as usize).min(64)
    } else {
        1
    }
}

/// Direct unpack sessions (threads, with their decoder state and open volumes) expected at
/// once for `capacity` slots: twice the slots, at least four. Sessions waiting for volumes
/// hold no slot, so there may be more of them than slots; more than this is logged.
pub fn session_limit_for(capacity: usize) -> usize {
    (capacity * 2).max(4)
}

/// [`session_limit_for`] the slots now.
pub fn session_limit() -> usize {
    session_limit_for(capacity())
}

/// Slots now, from the active options (read each time, so a reload applies).
pub fn capacity() -> usize {
    let option = crate::options::active_options().map_or(0, |o| o.scalars().unpack_slots);
    capacity_for(option)
}

/// The class of a download's direct unpack: [`Class::Paused`] when the files it still needs
/// cannot arrive for now (`remaining_data` files other than par2 left and every one of them
/// paused, `paused_data`; or `downloads_paused`: every download paused or the quota reached,
/// and it is not forced) or its fleet holds it (`held`), else [`Class::Active`]. Paused extra
/// par2 files left over do not count: such a download has everything to extract.
pub fn download_class(remaining_data: usize, paused_data: usize, downloads_paused: bool, held: bool) -> Class {
    let all_paused = remaining_data > 0 && paused_data >= remaining_data;
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

/// Whether one more finished job may enter its unpack stage now: post jobs holding or
/// waiting for a slot, and `starting` post jobs already in their unpack stage but not yet
/// holding or waiting for one, are fewer than the slots. Direct unpacks holding or waiting
/// are not counted: the job's unpack then waits for a slot in line ([`acquire`], class
/// [`Class::Finished`], aging), instead of being skipped for as long as some extraction
/// holds a slot (with one slot a live extraction takes it again for each article it
/// decodes, so a skipped job could wait for the whole download).
pub fn room_for_finished(starting: usize) -> bool {
    room_for_finished_in(capacity(), &lock(), starting)
}

fn room_for_finished_in(cap: usize, st: &State, starting: usize) -> bool {
    let posts = st.holders.iter().filter(|h| h.kind == Kind::Post).count()
        + st.waiters.iter().filter(|w| w.kind == Kind::Post).count();
    posts + starting < cap
}

/// How long a finished job's unpack waits before a direct unpack of any class gives its slot
/// up for it at the next safe point (two aging steps: it reached [`Class::Active`] one step
/// before and still found no slot).
pub const OVERDUE: Duration = Duration::from_secs(2 * AGE_STEP.as_secs());

/// Whether a direct unpack of class `own` holding a slot should give it up at a safe point
/// for one of `waiters` (class, kind, waited): one of a greater class, or a finished job's
/// unpack that waited [`OVERDUE`].
pub fn yields_to(own: Class, waiters: &[(Class, Kind, Duration)]) -> bool {
    waiters
        .iter()
        .any(|&(class, kind, waited)| class > own || (kind == Kind::Post && waited >= OVERDUE))
}

/// Whether someone waits for a slot (cheap; asked before the class of a holder is worked out).
pub fn anyone_waiting() -> bool {
    !lock().waiters.is_empty()
}

/// [`yields_to`] for the waiters now: whether the direct unpack holding `slot`, of class
/// `own` now, should give it up at its next safe point (between volumes or members, where it
/// would also stop to wait for a volume) and ask again ([`acquire`]), which puts it behind
/// that waiter.
pub fn should_yield(slot: &Slot, own: Class) -> bool {
    let st = lock();
    if !st.holders.iter().any(|h| h.ticket == slot.ticket && h.kind == Kind::Direct) {
        return false;
    }
    let waiters: Vec<(Class, Kind, Duration)> = st
        .waiters
        .iter()
        .map(|w| (w.class, w.kind, w.since.elapsed()))
        .collect();
    drop(st);
    yields_to(own, &waiters)
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

/// How long a waiter waits before it goes up one class: a finished job's unpack (or a paused
/// download's extraction) is not held back for ever by active downloads coming and going.
const AGE_STEP: Duration = Duration::from_secs(30);

/// `class` raised one step for every [`AGE_STEP`] waited (`waited`), up to [`Class::Active`].
pub fn aged(class: Class, waited: Duration) -> Class {
    let steps = (waited.as_secs() / AGE_STEP.as_secs()) as u8;
    match (class as u8).saturating_add(steps) {
        0 => Class::Paused,
        1 => Class::Finished,
        _ => Class::Active,
    }
}

/// Removes a waiter from the pool however its wait ends (given up, admitted, or a panic in
/// a callback), so no stale waiter holds others back.
struct WaiterGuard(u64);

impl Drop for WaiterGuard {
    fn drop(&mut self) {
        let mut st = lock();
        st.waiters.retain(|w| w.ticket != self.0);
        drop(st);
        CHANGED.notify_all();
    }
}

/// Takes a slot for an extraction, waiting while none is free for it. `class` is asked
/// before the first wait and again every quarter second while waiting (it may lock the queue:
/// it is never called with the pool's lock held); a waiter goes up one class for every
/// [`AGE_STEP`] it waited ([`aged`]) in this call (a direct unpack asks again after each
/// volume wait, so its aging starts over; a finished job's unpack asks once). `stopped` is asked as often: true gives up (None).
/// `on_wait(in_use, capacity)` is called once, when the first look found no slot.
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
            st.holders.push(Holder { ticket, kind });
            return Some(Slot { ticket });
        }
        ticket
    };
    let _guard = WaiterGuard(ticket);
    let since = std::time::Instant::now();
    let mut told = false;
    loop {
        if stopped() {
            return None;
        }
        let my_class = aged(class(), since.elapsed());
        let cap = capacity();
        let mut st = lock();
        match st.waiters.iter_mut().find(|w| w.ticket == ticket) {
            Some(w) => w.class = my_class,
            None => st.waiters.push(Waiter {
                ticket,
                class: my_class,
                kind,
                since,
            }),
        }
        let free = cap.saturating_sub(st.holders.len());
        let order: Vec<(Class, u64)> = st.waiters.iter().map(|w| (w.class, w.ticket)).collect();
        if admitted(free, &order, (my_class, ticket)) {
            st.waiters.retain(|w| w.ticket != ticket);
            st.holders.push(Holder { ticket, kind });
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
    fn capacity_is_one_unless_set() {
        assert_eq!(capacity_for(0), 1);
        assert_eq!(capacity_for(-1), 1);
        assert_eq!(capacity_for(1), 1);
        assert_eq!(capacity_for(8), 8);
        assert_eq!(capacity_for(1000), 64);
    }

    #[test]
    fn sessions_are_bounded_at_twice_the_slots_at_least_four() {
        assert_eq!(session_limit_for(1), 4);
        assert_eq!(session_limit_for(2), 4);
        assert_eq!(session_limit_for(4), 8);
        assert_eq!(session_limit_for(64), 128);
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
        // some data files still download
        assert_eq!(download_class(5, 1, false, false), Active);
        assert_eq!(download_class(5, 0, false, false), Active);
        // every data file arrived (paused extra par2 files are not counted): it finishes what
        // it has, as an active download
        assert_eq!(download_class(0, 0, false, false), Active);
        // every download paused (or the quota reached), and it is not forced
        assert_eq!(download_class(5, 0, true, false), Paused);
        // held by its fleet
        assert_eq!(download_class(5, 0, false, true), Paused);
    }

    #[test]
    fn a_long_wait_raises_a_waiter_one_class_every_half_minute() {
        use Class::*;
        let s = Duration::from_secs;
        assert_eq!(aged(Finished, s(0)), Finished);
        assert_eq!(aged(Finished, s(29)), Finished);
        assert_eq!(aged(Finished, s(30)), Active);
        assert_eq!(aged(Paused, s(30)), Finished);
        assert_eq!(aged(Paused, s(60)), Active);
        assert_eq!(aged(Active, s(600)), Active);
        // a finished job that waited goes before an active download asking now (older ticket)
        assert!(admitted(1, &[(aged(Finished, s(31)), 1), (Active, 9)], (Active, 1)));
    }

    fn state(holders: &[Kind], waiters: &[(Class, Kind)]) -> State {
        State {
            next_ticket: 100,
            holders: holders
                .iter()
                .enumerate()
                .map(|(i, &kind)| Holder {
                    ticket: i as u64,
                    kind,
                })
                .collect(),
            waiters: waiters
                .iter()
                .enumerate()
                .map(|(i, &(class, kind))| Waiter {
                    ticket: 50 + i as u64,
                    class,
                    kind,
                    since: std::time::Instant::now(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_finished_job_enters_its_unpack_unless_other_finished_unpacks_fill_the_slots() {
        use Class::*;
        use Kind::*;
        assert!(room_for_finished_in(1, &state(&[], &[]), 0));
        // a direct unpack holding the one slot (a live extraction) does not keep it out:
        // the job then waits for the slot in line, where it ages
        assert!(room_for_finished_in(1, &state(&[Direct], &[]), 0));
        assert!(room_for_finished_in(1, &state(&[Direct], &[(Active, Direct)]), 0));
        assert!(room_for_finished_in(1, &state(&[Direct], &[(Paused, Direct)]), 0));
        // another finished job's unpack holding or waiting does
        assert!(!room_for_finished_in(1, &state(&[Post], &[]), 0));
        assert!(!room_for_finished_in(1, &state(&[Direct], &[(Finished, Post)]), 0));
        assert!(room_for_finished_in(2, &state(&[Post], &[]), 0));
        // a job already entering its unpack counts
        assert!(!room_for_finished_in(1, &state(&[], &[]), 1));
        assert!(!room_for_finished_in(2, &state(&[Post], &[]), 1));
    }

    #[test]
    fn a_holder_gives_way_to_a_greater_class_or_an_overdue_finished_unpack() {
        use Class::*;
        use Kind::*;
        let s = Duration::from_secs;
        // nobody waiting
        assert!(!yields_to(Paused, &[]));
        // a paused or held download's extraction gives way to an active download and to a
        // finished job
        assert!(yields_to(Paused, &[(Active, Direct, s(0))]));
        assert!(yields_to(Paused, &[(Finished, Post, s(0))]));
        assert!(!yields_to(Paused, &[(Paused, Direct, s(0))]));
        // the active download's extraction goes first: a finished job waits until it idles
        // (between articles) or is overdue
        assert!(!yields_to(Active, &[(Finished, Post, s(0))]));
        assert!(!yields_to(Active, &[(Active, Post, s(59))]));
        assert!(yields_to(Active, &[(Active, Post, OVERDUE)]));
        // a paused download that waited long does not interrupt an active one
        assert!(!yields_to(Active, &[(Active, Direct, s(600))]));
    }
}
