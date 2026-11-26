//! F16: a fleet stays in Downloads. While a fleet of copies grouped by name
//! (a `dupes:` key, see [`crate::dupesearch_coord::is_group_key`]) is open,
//! every copy stays in the queue: one downloads (the primary), the others are
//! *held*: paused, at the lowest priority, never downloaded in full, and still
//! lending articles to the primary (the donor search reads queued copies).
//! Only when the primary succeeds do the others go to history (DELETED/DUPE,
//! nzb kept); when every copy failed the group goes to history as failures.
//!
//! F16a: the order the held copies are failed over in is computed here, once
//! ([`failover_order`]), and written on every member as [`ORDER_PARAM`] and
//! [`PRIMARY_PARAM`], so the web UI shows exactly the order failover picks
//! ([`next_held_host`] takes the first live copy of the same order).

use crate::status::{DeleteStatus, DupeMode};

/// `yes` on a held copy: paused at the lowest priority by its fleet.
pub const HELD_PARAM: &[u8] = b"DupeHeld";
/// The priority a copy had before it was held, restored when it is released.
pub const HELD_PRIORITY_PARAM: &[u8] = b"DupeHeldPriority";
/// `yes` while the fleet's background probe has not classified the copy yet.
pub const PROBING_PARAM: &[u8] = b"DupeProbing";
/// `yes` on every copy a fleet grouped by name added, from the moment it is
/// added: durable, never cleared (a restart in the middle of a probe clears
/// `DupeProbing`, and `DupeFleet` is written only when the copy is ranked).
pub const MEMBER_PARAM: &[u8] = b"DupeFleetMember";
/// The copy's place in its fleet: `0` the primary (downloading), `1` the copy
/// failover tries next, then `2`, `3`, ...
pub const ORDER_PARAM: &[u8] = b"DupeFleetOrder";
/// The NZBID of the fleet's primary (`0` while none downloads).
pub const PRIMARY_PARAM: &[u8] = b"DupeFleetPrimary";
/// How easy the copy is to borrow into ([`crate::donorkind::primary_ease`]):
/// `0` bare or split, `1` stored archive, `2` compressed, `3` encrypted.
pub const EASE_PARAM: &[u8] = b"DupeEase";
/// The priority of a held copy: the lowest NZBGet knows ("very low").
pub const HELD_PRIORITY: i32 = -100;
/// The ids of the files a hold paused (comma separated): only these are
/// resumed on release, so files the user paused and extra par2 volumes stay.
pub const HELD_FILES_PARAM: &[u8] = b"DupeHeldFiles";
/// F2c: the priority the fleet gave its primary so it downloads first (the
/// scheduler takes the highest priority before the queue order, so the front
/// of the queue alone loses to any item of a higher priority).
pub const BOOST_PARAM: &[u8] = b"DupeFleetBoost";
/// The priority the primary had before its fleet raised it ([`BOOST_PARAM`]),
/// given back when the copy is held again or leaves its fleet.
pub const OWN_PRIORITY_PARAM: &[u8] = b"DupeFleetOwnPriority";
/// The highest priority a fleet gives its primary: just under force (900),
/// which would download it even while downloads are paused.
pub const PRIMARY_PRIORITY_MAX: i32 = 899;

/// What the failover order is computed from, per held copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemberFacts {
    /// probed alive share in permille, `-1` unverified
    pub alive: i32,
    /// tried and failed its health check (kept as a donor): never failed over to
    pub parked: bool,
    /// archive headers encrypted and no password
    pub locked: bool,
    /// a twin of the fleet (same files by par2 lists)
    pub twin: bool,
    /// [`crate::donorkind::DonorKind`] as `u8` (borrowable before unknown before unborrowable)
    pub kind: u8,
    /// [`EASE_PARAM`]
    pub ease: u8,
    pub score: i32,
    pub id: i32,
}

impl MemberFacts {
    /// Whether the copy can still complete the release: alive or unverified,
    /// never tried and failed.
    pub fn live(&self) -> bool {
        !self.parked && self.alive != 0
    }

    /// Whether failover picks the copy: live and extractable (a locked copy
    /// is refused, its archive needs a password nobody has).
    pub fn pickable(&self) -> bool {
        self.live() && !self.locked
    }

    /// healthy, weak, unverified, locked, dead (the user's rule, F16a: a
    /// locked copy is never failed over to, so it goes after every copy that is)
    fn tier(&self) -> u8 {
        if !self.live() {
            4
        } else if self.locked {
            3
        } else if self.alive < 0 {
            2
        } else if self.alive < crate::dupesearch_nzb::FLEET_HEALTHY_ALIVE_PERMILLE {
            1
        } else {
            0
        }
    }
}

/// The failover order of a fleet's held copies (indices into `members`):
/// probed alive share first (healthy copies, at least 97.5%, before weak
/// ones), then the ease of borrowing into (bare or split, stored, compressed,
/// encrypted: F10), twins before alts; then unverified copies; locked copies
/// after every extractable one; dead or parked copies last. Weak copies
/// go by their alive share first: a 90% copy completes where a 40% one can't.
pub fn failover_order(members: &[MemberFacts]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..members.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&members[a], &members[b]);
        let tier = x.tier().cmp(&y.tier());
        let rest = match x.tier() {
            0 => x
                .ease
                .cmp(&y.ease)
                .then_with(|| y.twin.cmp(&x.twin))
                .then_with(|| x.kind.cmp(&y.kind))
                .then_with(|| y.alive.cmp(&x.alive)),
            1 => y
                .alive
                .cmp(&x.alive)
                .then_with(|| x.ease.cmp(&y.ease))
                .then_with(|| y.twin.cmp(&x.twin))
                .then_with(|| x.kind.cmp(&y.kind)),
            _ => y
                .twin
                .cmp(&x.twin)
                .then_with(|| x.kind.cmp(&y.kind))
                .then_with(|| y.alive.cmp(&x.alive)),
        };
        tier.then(rest)
            .then_with(|| y.score.cmp(&x.score))
            .then_with(|| x.id.cmp(&y.id))
    });
    order
}

/// The label the failover position shows: `next` for the first, then the place.
pub fn position_label(order: i32) -> String {
    match order {
        0 => "primary".to_string(),
        1 => "next".to_string(),
        n => n.to_string(),
    }
}

/// The order the held copies are shown in (indices into `held`, which is in
/// [`failover_order`], each with whether it is still being probed): the
/// copies failover can pick now first, in that order, so place 1 ("next") is
/// the copy failover takes; then copies still being probed, locked copies
/// and dead ones, in failover order.
pub fn display_order(held: &[(MemberFacts, bool)]) -> Vec<usize> {
    let ready = |i: &usize| held[*i].0.pickable() && !held[*i].1;
    let mut out: Vec<usize> = (0..held.len()).filter(ready).collect();
    out.extend((0..held.len()).filter(|i| !ready(i)));
    out
}

/// The place written as [`ORDER_PARAM`] for each held copy, in
/// [`display_order`] (indices into `held` with their place). Place 1
/// ("next") goes only to a copy failover can take now: while every held copy
/// is still being probed, locked or dead, the places start at 2, so a copy
/// still being probed is never shown as next, not even for one refresh (B5).
pub fn display_places(held: &[(MemberFacts, bool)]) -> Vec<(usize, i32)> {
    let ready = |i: usize| held[i].0.pickable() && !held[i].1;
    let order = display_order(held);
    let first = if order.first().is_some_and(|&i| ready(i)) { 1 } else { 2 };
    order
        .into_iter()
        .enumerate()
        .map(|(pos, i)| (i, first + pos as i32))
        .collect()
}

/// What a held copy becomes when its fleet ends with no live copy left: a
/// failure (dead: nothing of it alive) or a donor kept in history
/// (DELETED/DUPE, F3: any article alive, or not known, or locked).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    Failure,
    Donor,
}

pub fn ending_of(f: &MemberFacts) -> Ending {
    if f.alive == 0 {
        Ending::Failure
    } else {
        Ending::Donor
    }
}

/// Whether a fleet ended with no live copy: nothing downloads, nothing is
/// being probed (neither the key nor a held copy), held copies are left and
/// failover can pick none of them.
pub fn fleet_ended(active: usize, probing: bool, held: &[(MemberFacts, bool)]) -> bool {
    active == 0 && !probing && !held.is_empty() && !held.iter().any(|(f, p)| *p || f.pickable())
}

/// The priority a copy leaving its hold gets: the user's own when they
/// changed it while it was held, else the one it had before.
pub fn released_priority(current: i32, stored: Option<i32>) -> i32 {
    if current != HELD_PRIORITY {
        current
    } else {
        stored.unwrap_or(0)
    }
}

/// F2c: the priority a fleet's primary needs to download before every other
/// queued item, or None when it keeps its own. `current` is its priority now,
/// `boost` the one the fleet gave it last ([`BOOST_PARAM`], None when it never
/// did) and `others` the priorities of the queued items it competes with (not
/// held, not another fleet's primary). The scheduler takes the highest
/// priority first and the queue order only among equals, so the primary goes
/// one above the highest, never to force (900 and up, which downloads even
/// while downloads are paused; an item there keeps winning). A priority the
/// user set after the fleet raised it stays theirs.
pub fn primary_priority(current: i32, boost: Option<i32>, others: &[i32]) -> Option<i32> {
    if boost.is_some_and(|b| b != current) {
        return None;
    }
    let top = others.iter().copied().filter(|&p| p <= PRIMARY_PRIORITY_MAX).max()?;
    let want = top.saturating_add(1).min(PRIMARY_PRIORITY_MAX);
    (want > current).then_some(want)
}

/// The priority to keep for a copy its fleet raised ([`primary_priority`]):
/// its own from before while the raised one is still in place, else the
/// current one (the user changed it since).
pub fn unboosted_priority(current: i32, boost: Option<i32>, own: Option<i32>) -> i32 {
    match (boost, own) {
        (Some(b), Some(o)) if b == current => o,
        _ => current,
    }
}

/// [`HELD_FILES_PARAM`] from file ids, and back.
pub fn held_files_param(ids: &[i32]) -> String {
    ids.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

pub fn parse_held_files(value: &[u8]) -> Vec<i32> {
    std::str::from_utf8(value)
        .unwrap_or("")
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect()
}

/// `yes` on a copy that leaves its fleet without its files deleted, whatever
/// the delete mode says (a copy sharing the primary's folder, or held copies
/// a fleet ends with): `prepost::delete_cleanup_host` leaves its disk alone.
pub const KEEP_FILES_PARAM: &[u8] = b"DupeKeepFiles";

/// Whether two download folders are one: the same path, or the same folder
/// once resolved (links, `..`), or equal ignoring case on macOS, whose file
/// systems are case-insensitive by default.
pub fn same_folder(a: &[u8], b: &[u8]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let resolve = |p: &[u8]| std::fs::canonicalize(std::ffi::OsStr::new(std::str::from_utf8(p).ok()?)).ok();
    if let (Some(x), Some(y)) = (resolve(a), resolve(b)) {
        if x == y {
            return true;
        }
    }
    let trim = |p: &[u8]| -> Vec<u8> {
        let mut v = p.to_vec();
        while v.len() > 1 && v.last() == Some(&b'/') {
            v.pop();
        }
        v
    };
    let (a, b) = (trim(a), trim(b));
    a == b || (cfg!(target_os = "macos") && a.eq_ignore_ascii_case(&b))
}

/// Keys whose fleet is being ended or refreshed now: a nested end or refresh
/// of the same key (the queue editor runs a deleted copy's completion inline,
/// which refreshes its fleet) does nothing, so the outer walk keeps its copies.
static BUSY_KEYS: std::sync::Mutex<Vec<Vec<u8>>> = std::sync::Mutex::new(Vec::new());

/// Holds a key busy while it lives; None when the key is busy already.
pub struct BusyKey(Vec<u8>);

impl BusyKey {
    pub fn enter(key: &[u8]) -> Option<Self> {
        let key = key.to_ascii_lowercase();
        let mut keys = BUSY_KEYS.lock().unwrap_or_else(|e| e.into_inner());
        if keys.contains(&key) {
            return None;
        }
        keys.push(key.clone());
        Some(Self(key))
    }
}

impl Drop for BusyKey {
    fn drop(&mut self) {
        let mut keys = BUSY_KEYS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = keys.iter().position(|k| *k == self.0) {
            keys.remove(pos);
        }
    }
}

/// Keys whose background probe runs now (see [`ProbingGuard`]): their
/// `DupeProbing` copies are not stale.
static PROBING_KEYS: std::sync::Mutex<Vec<Vec<u8>>> = std::sync::Mutex::new(Vec::new());

/// Marks a key as probed in the background while it lives.
pub struct ProbingGuard(Vec<u8>);

impl ProbingGuard {
    pub fn enter(key: &[u8]) -> Self {
        let key = key.to_ascii_lowercase();
        PROBING_KEYS.lock().unwrap_or_else(|e| e.into_inner()).push(key.clone());
        Self(key)
    }
}

impl Drop for ProbingGuard {
    fn drop(&mut self) {
        let mut keys = PROBING_KEYS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = keys.iter().position(|k| *k == self.0) {
            keys.remove(pos);
        }
    }
}

/// Whether the key's background probe runs now.
pub fn probing(key: &[u8]) -> bool {
    let key = key.to_ascii_lowercase();
    PROBING_KEYS.lock().unwrap_or_else(|e| e.into_inner()).contains(&key)
}

#[cfg(unix)]
pub(crate) use host::*;

#[cfg(unix)]
mod host {
    use super::*;
    use crate::queuesubsys_host::{NzbRef, NzbSnap, QueueSubsysHost};

    fn flag(host: &QueueSubsysHost, ptr: NzbRef, name: &[u8]) -> bool {
        host.nzb_get_param(ptr, name)
            .is_some_and(|v| v.trim_ascii().eq_ignore_ascii_case(b"yes"))
    }

    /// Whether the copy is held by its fleet.
    pub(crate) fn is_held(host: &QueueSubsysHost, ptr: NzbRef) -> bool {
        flag(host, ptr, HELD_PARAM)
    }

    /// Whether the fleet's probe has not classified the copy yet.
    pub(crate) fn is_probing(host: &QueueSubsysHost, ptr: NzbRef) -> bool {
        flag(host, ptr, PROBING_PARAM)
    }

    /// Whether a download's fleet keeps its copies in the queue (F16): copies
    /// grouped by name, in DupeMode SCORE, with the duplicate check on.
    pub(crate) fn holds_in_queue(host: &QueueSubsysHost, nzb: &NzbSnap) -> bool {
        nzb.kind == crate::queuesubsys_host::NK_NZB
            && nzb.dupe_mode == DupeMode::Score
            && crate::dupesearch_coord::is_group_key(&nzb.dupe_key)
            && host.opts().dupe_check
    }

    /// The queued copies of a fleet key (not being deleted).
    pub(crate) fn queued_members(host: &QueueSubsysHost, dq_ptr: usize, key: &[u8]) -> Vec<NzbSnap> {
        queue_snaps(host, dq_ptr)
            .into_iter()
            .filter(|q| is_member(q, key))
            .collect()
    }

    /// Every queued item's snapshot, in queue order.
    fn queue_snaps(host: &QueueSubsysHost, dq_ptr: usize) -> Vec<NzbSnap> {
        (0..host.queue_len(dq_ptr))
            .map(|i| host.queue_at(dq_ptr, i))
            .filter(|qp| qp.is_some())
            .map(|qp| host.nzb_snap(qp, false))
            .collect()
    }

    /// Whether a queued item is a copy of the fleet `key` (not being deleted).
    fn is_member(q: &NzbSnap, key: &[u8]) -> bool {
        q.kind == crate::queuesubsys_host::NK_NZB
            && !q.deleting
            && q.delete_status == DeleteStatus::None
            && q.dupe_key.eq_ignore_ascii_case(key)
    }

    pub(crate) fn facts_of(host: &QueueSubsysHost, q: &NzbSnap) -> MemberFacts {
        let kind = crate::donorkind::DonorKind::from_param(
            host.nzb_get_param(q.ptr, crate::donorkind::KIND_PARAM).as_deref(),
        );
        let twin_label = host
            .nzb_get_param(q.ptr, b"DupeKind")
            .is_some_and(|v| v.starts_with(b"twin"));
        MemberFacts {
            alive: crate::dupecoord::alive_permille_host(host, q.ptr),
            parked: host.nzb_get_param(q.ptr, crate::dupecoord::PARKED_PARAM).is_some(),
            locked: crate::dupecoord::locked_host(host, q.ptr),
            twin: kind == crate::donorkind::DonorKind::Twin || twin_label,
            kind: kind as u8,
            ease: host
                .nzb_get_param(q.ptr, EASE_PARAM)
                .and_then(|v| std::str::from_utf8(&v).ok()?.trim().parse::<u8>().ok())
                .unwrap_or(2),
            score: q.dupe_score,
            id: q.id,
        }
    }

    /// The held copies of `key` in failover order, with their facts; `exclude`
    /// is left out (the copy failing over).
    pub(crate) fn held_in_order(
        host: &QueueSubsysHost,
        dq_ptr: usize,
        key: &[u8],
        exclude: NzbRef,
    ) -> Vec<(NzbSnap, MemberFacts)> {
        held_in_order_of(host, &queued_members(host, dq_ptr, key), exclude)
    }

    /// [`held_in_order`] among `members`, the fleet's queued copies.
    fn held_in_order_of(host: &QueueSubsysHost, members: &[NzbSnap], exclude: NzbRef) -> Vec<(NzbSnap, MemberFacts)> {
        let held: Vec<&NzbSnap> = members
            .iter()
            .filter(|q| q.ptr != exclude && is_held(host, q.ptr))
            .collect();
        let facts: Vec<MemberFacts> = held.iter().map(|q| facts_of(host, q)).collect();
        failover_order(&facts)
            .into_iter()
            .map(|i| (held[i].clone(), facts[i]))
            .collect()
    }

    /// The held copy failover picks next for `nzb`'s fleet: the first live one
    /// in [`failover_order`]; never one still being probed.
    pub(crate) fn next_held_host(
        host: &QueueSubsysHost,
        dq_ptr: usize,
        nzb: &NzbSnap,
    ) -> Option<NzbSnap> {
        if !holds_in_queue(host, nzb) {
            return None;
        }
        held_in_order(host, dq_ptr, &nzb.dupe_key, nzb.ptr)
            .into_iter()
            .find(|(q, f)| f.pickable() && !is_probing(host, q.ptr))
            .map(|(q, _)| q)
    }

    /// Pauses or resumes every file of a copy: through the queue editor when
    /// the copy is in the queue (its paused sizes follow), directly on its
    /// files while it is still being added (the duplicate check of a new copy
    /// runs before it is in the queue).
    fn set_paused(host: &QueueSubsysHost, dq_ptr: usize, q: &NzbSnap, paused: bool) {
        if host.queue_find_by_id(dq_ptr, q.id).is_some() {
            let action = if paused {
                crate::queuesubsys_host::EA_GROUP_PAUSE
            } else {
                crate::queuesubsys_host::EA_GROUP_RESUME
            };
            host.dq_edit_entry(dq_ptr, q.id, action, None);
        } else {
            for i in 0..host.nzb_file_len(q.ptr) {
                let f = host.nzb_file_at(q.ptr, i);
                host.file_set_int(f, crate::queuesubsys_host::FILE_INT_PAUSED, i32::from(paused));
            }
        }
    }

    /// Whether a copy of `key` other than `exclude` downloads (is queued, not held).
    pub(crate) fn has_active(host: &QueueSubsysHost, dq_ptr: usize, key: &[u8], exclude: NzbRef) -> bool {
        queued_members(host, dq_ptr, key)
            .iter()
            .any(|q| q.ptr != exclude && !is_held(host, q.ptr))
    }

    fn int_param(host: &QueueSubsysHost, ptr: NzbRef, name: &[u8]) -> Option<i32> {
        host.nzb_get_param(ptr, name)
            .and_then(|v| std::str::from_utf8(&v).ok()?.trim().parse::<i32>().ok())
    }

    /// The copy's priority without its fleet's raise ([`unboosted_priority`]),
    /// the raise's marks gone.
    fn drop_boost(host: &QueueSubsysHost, ptr: NzbRef, current: i32) -> i32 {
        let own = unboosted_priority(
            current,
            int_param(host, ptr, BOOST_PARAM),
            int_param(host, ptr, OWN_PRIORITY_PARAM),
        );
        host.nzb_set_param(ptr, BOOST_PARAM, None);
        host.nzb_set_param(ptr, OWN_PRIORITY_PARAM, None);
        own
    }

    /// A primary whose fleet ends (it succeeded, or it failed and goes to
    /// history) gets its own priority back and loses the raise's marks, so
    /// post-processing order, History and a later return from it never see
    /// the raise.
    pub(crate) fn unboost_host(host: &QueueSubsysHost, ptr: NzbRef) {
        if host.nzb_get_param(ptr, BOOST_PARAM).is_none() {
            return;
        }
        let current = host.nzb_snap(ptr, false).priority;
        let own = drop_boost(host, ptr, current);
        host.nzb_set_int(ptr, crate::queuesubsys_host::NZB_INT_PRIORITY, own);
        host.nzb_set_int(ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
    }

    /// A queued item as a fleet's primary competes with it for the download
    /// ([`boost_primary_with`]).
    struct Rival {
        ptr: NzbRef,
        key: Vec<u8>,
        priority: i32,
        competes: bool,
    }

    /// The queued items the primaries compete with, read once for a pass over
    /// every fleet ([`refresh_all_host`]) rather than once per primary.
    struct Rivals(Vec<Rival>);

    /// Whether a queued item competes for the download with the primary of a
    /// fleet other than its own: not a held copy, not another fleet's primary
    /// (two fleets would raise each other in turn), not an item paused whole.
    fn competes(host: &QueueSubsysHost, o: &NzbSnap) -> bool {
        o.kind == crate::queuesubsys_host::NK_NZB
            && !o.deleting
            && o.delete_status == DeleteStatus::None
            && !is_held(host, o.ptr)
            && host.nzb_get_param(o.ptr, BOOST_PARAM).is_none()
            // another fleet's primary by its role, raised or not: two
            // fleets keep their own order and never leapfrog
            && !(holds_in_queue(host, o) && host.nzb_get_param(o.ptr, ORDER_PARAM).as_deref() == Some(b"0"))
            && host.nzb_file_len(o.ptr) as i32 != o.paused_file_count
    }

    impl Rivals {
        fn of(host: &QueueSubsysHost, snaps: &[NzbSnap]) -> Self {
            Rivals(
                snaps
                    .iter()
                    .map(|o| Rival {
                        ptr: o.ptr,
                        key: o.dupe_key.clone(),
                        priority: o.priority,
                        competes: competes(host, o),
                    })
                    .collect(),
            )
        }

        fn scan(host: &QueueSubsysHost, dq_ptr: usize) -> Self {
            Self::of(host, &queue_snaps(host, dq_ptr))
        }

        /// The priorities the primary `ptr` of fleet `key` competes with.
        fn others(&self, key: &[u8], ptr: NzbRef) -> Vec<i32> {
            self.0
                .iter()
                .filter(|r| r.competes && r.ptr != ptr && !r.key.eq_ignore_ascii_case(key))
                .map(|r| r.priority)
                .collect()
        }

        /// Reads `ptrs` again after their fleet's refresh changed them (a
        /// raise, a release, a new place).
        fn update(&mut self, host: &QueueSubsysHost, ptrs: &[NzbRef]) {
            for r in self.0.iter_mut().filter(|r| ptrs.contains(&r.ptr)) {
                let o = host.nzb_snap(r.ptr, false);
                r.priority = o.priority;
                r.competes = competes(host, &o);
            }
        }

        /// Drops `ptrs` after their fleet ended: its copies left the queue
        /// and may be freed already, so they are never read again.
        fn forget(&mut self, ptrs: &[NzbRef]) {
            for r in self.0.iter_mut().filter(|r| ptrs.contains(&r.ptr)) {
                r.competes = false;
            }
        }
    }

    #[cfg(test)]
    mod rival_tests {
        use super::*;

        fn rival(addr: usize, key: &[u8], priority: i32, competes: bool) -> Rival {
            Rival { ptr: NzbRef::from_addr(addr), key: key.to_vec(), priority, competes }
        }

        #[test]
        fn a_primary_competes_with_other_fleets_and_plain_items_only() {
            let mut rivals = Rivals(vec![
                rival(1, b"dupes:a", 50, true),  // the primary itself
                rival(2, b"DUPES:A", 70, true),  // its own fleet, any case
                rival(3, b"dupes:b", 20, true),  // another fleet's item
                rival(4, b"", 10, true),         // a plain item
                rival(5, b"dupes:c", 99, false), // held, raised or paused whole
            ]);
            assert_eq!(rivals.others(b"dupes:a", NzbRef::from_addr(1)), vec![20, 10]);
            // an ended fleet's copies never count (nor are they read) again
            rivals.forget(&[NzbRef::from_addr(3)]);
            assert_eq!(rivals.others(b"dupes:a", NzbRef::from_addr(1)), vec![10]);
            assert_eq!(rivals.others(b"dupes:b", NzbRef::from_addr(3)), vec![50, 70, 10]);
        }
    }

    /// F2c: raises a downloading copy of `key` (its primary) above every other
    /// queued item ([`primary_priority`]), so it downloads at once whatever
    /// the priorities of the rest of the queue. `rivals` are the queued items it
    /// competes with. Returns whether it changed.
    fn boost_primary_with(host: &QueueSubsysHost, rivals: &Rivals, key: &[u8], ptr: NzbRef) -> bool {
        let q = host.nzb_snap(ptr, false);
        let others = rivals.others(key, ptr);
        let boost = int_param(host, ptr, BOOST_PARAM);
        let Some(want) = primary_priority(q.priority, boost, &others) else {
            return false;
        };
        if boost.is_none() {
            host.nzb_set_param(ptr, OWN_PRIORITY_PARAM, Some(q.priority.to_string().as_bytes()));
        }
        host.nzb_set_param(ptr, BOOST_PARAM, Some(want.to_string().as_bytes()));
        host.nzb_set_int(ptr, crate::queuesubsys_host::NZB_INT_PRIORITY, want);
        let mut msg = b"Raising ".to_vec();
        msg.extend_from_slice(&q.name);
        msg.extend_from_slice(
            format!(" to priority {want}, above every other queued item: the primary of its fleet downloads first")
                .as_bytes(),
        );
        host.nzb_print_message(ptr, crate::queuesubsys_host::MK_INFO, &crate::releasename::log_name(&msg));
        host.nzb_set_int(ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
        true
    }

    /// Holds a queued copy: paused, at the lowest priority, marked [`HELD_PARAM`].
    /// A copy already in the queue records the files the hold paused
    /// ([`HELD_FILES_PARAM`]), so a release resumes only those.
    pub(crate) fn hold_host(host: &QueueSubsysHost, dq_ptr: usize, q: &NzbSnap, why: &[u8]) {
        if !is_held(host, q.ptr) {
            // a primary held again keeps its own priority, not its fleet's raise
            let own = drop_boost(host, q.ptr, q.priority);
            host.nzb_set_param(q.ptr, HELD_PRIORITY_PARAM, Some(own.to_string().as_bytes()));
            if host.queue_find_by_id(dq_ptr, q.id).is_some() {
                let mut paused_now = Vec::new();
                for i in 0..host.nzb_file_len(q.ptr) {
                    let f = host.file_snap(host.nzb_file_at(q.ptr, i));
                    if !f.paused && !f.deleted {
                        host.file_set_int(f.ptr, crate::queuesubsys_host::FILE_INT_PAUSED, 1);
                        paused_now.push(f.id);
                    }
                }
                host.nzb_set_param(q.ptr, HELD_FILES_PARAM, Some(held_files_param(&paused_now).as_bytes()));
            } else {
                // still being added: nothing of it is the user's yet; the
                // release resumes it as a fresh copy (extra pars paused)
                set_paused(host, dq_ptr, q, true);
            }
        }
        host.nzb_set_param(q.ptr, HELD_PARAM, Some(b"yes"));
        host.nzb_set_int(q.ptr, crate::queuesubsys_host::NZB_INT_PRIORITY, HELD_PRIORITY);
        if !why.is_empty() {
            let mut msg = b"Holding ".to_vec();
            msg.extend_from_slice(&q.name);
            msg.extend_from_slice(b" in its fleet: ");
            msg.extend_from_slice(why);
            host.nzb_print_message(q.ptr, crate::queuesubsys_host::MK_INFO, &crate::releasename::log_name(&msg));
        }
        host.nzb_set_int(q.ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
    }

    /// Takes a copy out of its fleet's hold: its own priority back (the
    /// user's, when they changed it meanwhile) and the hold's marks gone.
    /// Its files stay as they are.
    pub(crate) fn leave_fleet_host(host: &QueueSubsysHost, ptr: NzbRef) {
        let q = host.nzb_snap(ptr, false);
        let stored = int_param(host, ptr, HELD_PRIORITY_PARAM);
        if stored.is_some() || is_held(host, ptr) {
            host.nzb_set_int(ptr, crate::queuesubsys_host::NZB_INT_PRIORITY, released_priority(q.priority, stored));
        } else if host.nzb_get_param(ptr, BOOST_PARAM).is_some() {
            // a primary leaving its fleet gets its own priority back
            let own = drop_boost(host, ptr, q.priority);
            host.nzb_set_int(ptr, crate::queuesubsys_host::NZB_INT_PRIORITY, own);
        }
        for name in [HELD_PARAM, HELD_PRIORITY_PARAM, PROBING_PARAM, HELD_FILES_PARAM] {
            host.nzb_set_param(ptr, name, None);
        }
        host.nzb_set_int(ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
    }

    /// Releases a held copy: it downloads at its own priority again. Only the
    /// files the hold paused are resumed; a copy held as it was added resumes
    /// as a fresh copy does (every file, then the extra par2 volumes paused
    /// again unless ParCheck is Force).
    pub(crate) fn release_host(host: &QueueSubsysHost, dq_ptr: usize, q: &NzbSnap, why: &[u8]) {
        let held_files = host.nzb_get_param(q.ptr, HELD_FILES_PARAM).map(|v| parse_held_files(&v));
        leave_fleet_host(host, q.ptr);
        match held_files {
            Some(ids) => {
                for i in 0..host.nzb_file_len(q.ptr) {
                    let f = host.file_snap(host.nzb_file_at(q.ptr, i));
                    if ids.contains(&f.id) {
                        host.file_set_int(f.ptr, crate::queuesubsys_host::FILE_INT_PAUSED, 0);
                    }
                }
            }
            None => {
                set_paused(host, dq_ptr, q, false);
                if host.opts().par_check != crate::queuesubsys_host::PC_FORCE
                    && host.queue_find_by_id(dq_ptr, q.id).is_some()
                {
                    host.dq_edit_entry(dq_ptr, q.id, crate::queuesubsys_host::EA_GROUP_PAUSE_EXTRA_PARS, None);
                }
            }
        }
        let mut msg = b"Releasing ".to_vec();
        msg.extend_from_slice(&q.name);
        msg.extend_from_slice(b" from its fleet: ");
        msg.extend_from_slice(why);
        host.nzb_print_message(q.ptr, crate::queuesubsys_host::MK_INFO, &crate::releasename::log_name(&msg));
    }

    /// The share of a copy's tried articles that arrived, as kept on a parked donor.
    fn parked_alive_text(host: &QueueSubsysHost, nzb: &NzbSnap) -> String {
        let alive = host
            .nzb_get_param(nzb.ptr, crate::dupecoord::PROJECTED_PARAM)
            .and_then(|v| crate::dupecoord::parse_f64_prefix(&v))
            .unwrap_or_else(|| {
                crate::dupecoord::arrived_percent(
                    nzb.current_success_articles,
                    nzb.current_failed_articles,
                )
            })
            .clamp(0.0, 100.0);
        if alive <= 0.0 {
            "0".to_string()
        } else {
            format!("{alive:.1}")
        }
    }

    /// Parks a download that failed its health check (`EA_GROUP_PARK_DELETE`
    /// with DeleteStatus HEALTH), unless its fleet keeps it in the queue: then
    /// it is held as a donor with its measured alive share ([`crate::dupecoord::PARKED_PARAM`])
    /// and the next held copy in failover order downloads. When no live copy
    /// is left, the fleet failed: its held copies go to history as failures
    /// with it.
    pub(crate) fn park_on_health_host(host: &QueueSubsysHost, dq_ptr: usize, nzb: &NzbSnap) {
        // a download failed over or parked is no longer cut at its frontier: if its fleet
        // releases it again, it fetches its own articles
        crate::frontiercut::forget(nzb.id);
        // its compressed restarts too: their scratch files are not kept while it is parked
        crate::restartlend::forget(nzb.id);
        // a copy already held (failed over a moment ago: its last articles in
        // flight still report) is not failed over again: that would release a
        // second copy
        if holds_in_queue(host, nzb) && is_held(host, nzb.ptr) {
            return;
        }
        if holds_in_queue(host, nzb) {
            if let Some(next) = next_held_host(host, dq_ptr, nzb) {
                let alive = parked_alive_text(host, nzb);
                host.nzb_set_param(nzb.ptr, crate::dupecoord::PARKED_PARAM, Some(alive.as_bytes()));
                host.nzb_set_param(nzb.ptr, b"DupeAlive", Some(alive.as_bytes()));
                let why = if alive == "0" {
                    b"none of its tried articles arrived, kept as dead with its nzb".to_vec()
                } else {
                    format!("{alive}% of its tried articles arrived, kept as a donor").into_bytes()
                };
                hold_host(host, dq_ptr, nzb, &why);
                let mut msg = b"failing over from ".to_vec();
                msg.extend_from_slice(&nzb.name);
                release_host(host, dq_ptr, &next, &msg);
                refresh_key_host(host, dq_ptr, &nzb.dupe_key);
                host.dq_save_changed(dq_ptr);
                return;
            }
            if probing(&nzb.dupe_key) {
                // copies of another submission are still being probed: kept
                // as a donor; once the probe is over the fleet's refresh
                // releases the best of them, or ends the fleet
                let alive = parked_alive_text(host, nzb);
                host.nzb_set_param(nzb.ptr, crate::dupecoord::PARKED_PARAM, Some(alive.as_bytes()));
                host.nzb_set_param(nzb.ptr, b"DupeAlive", Some(alive.as_bytes()));
                hold_host(host, dq_ptr, nzb, b"failed its health check while other copies are still being probed");
                host.dq_save_changed(dq_ptr);
                return;
            }
            end_fleet_host(host, dq_ptr, &nzb.dupe_key, nzb.ptr);
        }
        unboost_host(host, nzb.ptr);
        host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Health);
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_PARK_DELETE,
            None,
        );
    }

    /// A fleet ended with no live copy (its last one failed: a health check,
    /// post-processing, HealthCheck=Delete or a delete by hand): its held
    /// copies go to history, nzbs and files kept: a dead one (nothing alive)
    /// as FAILURE/HEALTH, any other as a donor, DELETED/DUPE (F3). `exclude`
    /// is the copy that just failed. Copies still being probed are left to
    /// their probe.
    pub(crate) fn end_fleet_host(host: &QueueSubsysHost, dq_ptr: usize, key: &[u8], exclude: NzbRef) {
        let Some(_busy) = BusyKey::enter(key) else {
            return;
        };
        end_fleet_locked(host, dq_ptr, key, exclude);
    }

    /// [`end_fleet_host`] with the key held busy by the caller.
    fn end_fleet_locked(host: &QueueSubsysHost, dq_ptr: usize, key: &[u8], exclude: NzbRef) {
        // ids, not pointers: a delete can complete inline and, with
        // KeepHistory=0, free the copy before the next one is handled
        let ids: Vec<i32> = held_in_order(host, dq_ptr, key, exclude)
            .into_iter()
            .filter(|(q, _)| !is_probing(host, q.ptr))
            .map(|(q, _)| q.id)
            .collect();
        for id in ids {
            let ptr = host.queue_find_by_id(dq_ptr, id);
            if ptr.is_none() {
                continue;
            }
            let q = host.nzb_snap(ptr, false);
            if q.deleting || q.delete_status != DeleteStatus::None || !is_held(host, q.ptr) {
                continue;
            }
            let ending = ending_of(&facts_of(host, &q));
            let mut msg = b"Every copy of the fleet of ".to_vec();
            msg.extend_from_slice(&q.name);
            msg.extend_from_slice(match ending {
                Ending::Failure => b" failed: it goes to history as dead, its nzb kept".as_slice(),
                Ending::Donor => b" failed: it goes to history as a donor, its nzb kept".as_slice(),
            });
            host.nzb_print_message(q.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
            leave_fleet_host(host, q.ptr);
            match ending {
                Ending::Failure => {
                    if host.nzb_get_param(q.ptr, crate::dupecoord::PROJECTED_PARAM).is_none() {
                        host.nzb_set_param(q.ptr, crate::dupecoord::PROJECTED_PARAM, Some(b"0.0"));
                    }
                    host.nzb_set_delete_status(q.ptr, DeleteStatus::Health);
                }
                Ending::Donor => host.nzb_set_delete_status(q.ptr, DeleteStatus::Dupe),
            }
            // parked, its files and nzb kept whatever the delete mode says
            host.nzb_set_param(q.ptr, KEEP_FILES_PARAM, Some(b"yes"));
            host.dq_edit_entry(dq_ptr, id, crate::queuesubsys_host::EA_GROUP_PARK_DELETE, None);
        }
    }

    /// The primary of a fleet succeeded: every other queued copy goes to
    /// history as DELETED/DUPE (its nzb kept), whatever the HealthCheck mode.
    pub(crate) fn fleet_succeeded_host(host: &QueueSubsysHost, dq_ptr: usize, nzb: &NzbSnap) {
        unboost_host(host, nzb.ptr);
        if !holds_in_queue(host, nzb) {
            return;
        }
        let others: Vec<i32> = queued_members(host, dq_ptr, &nzb.dupe_key)
            .into_iter()
            .filter(|q| q.ptr != nzb.ptr && !q.has_post_info)
            .map(|q| q.id)
            .collect();
        for id in others {
            // re-found each time: a delete can complete inline and free a copy
            let ptr = host.queue_find_by_id(dq_ptr, id);
            if ptr.is_none() {
                continue;
            }
            let q = host.nzb_snap(ptr, false);
            if q.deleting || q.delete_status != DeleteStatus::None {
                continue;
            }
            let mut msg = b"Moving ".to_vec();
            msg.extend_from_slice(&q.name);
            msg.extend_from_slice(b" to history: ");
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b", a copy of its fleet, succeeded");
            host.nzb_print_message(q.ptr, crate::queuesubsys_host::MK_INFO, &crate::releasename::log_name(&msg));
            leave_fleet_host(host, q.ptr);
            host.nzb_set_delete_status(q.ptr, DeleteStatus::Dupe);
            // its own folder only: without InterDir two copies of one name
            // share a folder, and the files there may be the primary's release
            let shares_folder =
                same_folder(&q.dest_dir, &nzb.dest_dir) || same_folder(&q.dest_dir, &nzb.final_dir);
            if shares_folder {
                // never a disk clean-up: the folder's files may be the primary's
                host.nzb_set_param(q.ptr, KEEP_FILES_PARAM, Some(b"yes"));
            }
            host.dq_edit_entry(dq_ptr, q.id, crate::queuesubsys_host::EA_GROUP_DUPE_DELETE, None);
        }
    }

    fn set_if_changed(host: &QueueSubsysHost, ptr: NzbRef, name: &[u8], value: &[u8]) -> bool {
        if host.nzb_get_param(ptr, name).as_deref() == Some(value) {
            return false;
        }
        host.nzb_set_param(ptr, name, Some(value));
        true
    }

    /// Writes [`ORDER_PARAM`] and [`PRIMARY_PARAM`] on every queued copy of
    /// `key`; when no copy downloads, nothing is being probed and a live held
    /// copy waits, it is released (a primary deleted by hand, a restart in the
    /// middle of a probe). Returns whether anything changed.
    pub(crate) fn refresh_key_host(host: &QueueSubsysHost, dq_ptr: usize, key: &[u8]) -> bool {
        let members = queued_members(host, dq_ptr, key);
        refresh_members(host, dq_ptr, key, &members, &mut None) != Refresh::Unchanged
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Refresh {
        Unchanged,
        Changed,
        /// The fleet ended: its held copies went to history.
        Ended,
    }

    /// [`refresh_key_host`] with the fleet's queued copies read already;
    /// `rivals` is read when first needed if None.
    fn refresh_members(
        host: &QueueSubsysHost,
        dq_ptr: usize,
        key: &[u8],
        members: &[NzbSnap],
        rivals: &mut Option<Rivals>,
    ) -> Refresh {
        // nested inside an end or refresh of the same key: nothing to do
        let Some(_busy) = BusyKey::enter(key) else {
            return Refresh::Unchanged;
        };
        if members.is_empty() {
            return Refresh::Unchanged;
        }
        let changed_if = |c: bool| if c { Refresh::Changed } else { Refresh::Unchanged };
        let probing_now = probing(key);
        let mut changed = false;
        // a copy left probing by a probe that no longer runs (a restart)
        if !probing_now {
            for q in members {
                if is_probing(host, q.ptr) {
                    host.nzb_set_param(q.ptr, PROBING_PARAM, None);
                    changed = true;
                }
            }
        }
        let mut active: Vec<&NzbSnap> = members.iter().filter(|q| !is_held(host, q.ptr)).collect();
        active.sort_by_key(|q| std::cmp::Reverse(q.dupe_score));
        let mut ordered = held_in_order_of(host, members, NzbRef::NONE);
        if active.is_empty() && !probing_now {
            // never a locked copy: failover refuses it
            let first = ordered
                .iter()
                .position(|(q, f)| f.pickable() && !is_probing(host, q.ptr));
            let flags: Vec<(MemberFacts, bool)> =
                ordered.iter().map(|(q, f)| (*f, is_probing(host, q.ptr))).collect();
            if first.is_none() && fleet_ended(0, probing_now, &flags) {
                end_fleet_locked(host, dq_ptr, key, NzbRef::NONE);
                host.dq_save_changed(dq_ptr);
                return Refresh::Ended;
            }
            if let Some(pos) = first {
                let (q, _) = ordered.remove(pos);
                release_host(host, dq_ptr, &q, b"no copy of its fleet downloads");
                changed = true;
                let id = q.id;
                let snap = members.iter().find(|m| m.id == id).cloned();
                if let Some(snap) = snap {
                    return changed_if(finish_refresh(host, dq_ptr, &[&snap], &ordered, rivals) || changed);
                }
            }
        }
        changed_if(finish_refresh(host, dq_ptr, &active, &ordered, rivals) || changed)
    }

    fn finish_refresh(
        host: &QueueSubsysHost,
        dq_ptr: usize,
        active: &[&NzbSnap],
        ordered: &[(NzbSnap, MemberFacts)],
        rivals: &mut Option<Rivals>,
    ) -> bool {
        let primary = active.first().map_or(0, |q| q.id).to_string();
        let mut changed = false;
        for q in active {
            // F2c: the fleet's download goes before every other queued item,
            // also after a failover (each refresh looks again)
            if !q.has_post_info {
                let rivals = rivals.get_or_insert_with(|| Rivals::scan(host, dq_ptr));
                changed |= boost_primary_with(host, rivals, &q.dupe_key, q.ptr);
            } else if host.nzb_get_param(q.ptr, BOOST_PARAM).is_some() {
                // downloaded: post-processing (and History after it) sees its
                // own priority, whatever way it ends
                unboost_host(host, q.ptr);
                changed = true;
            }
            changed |= set_if_changed(host, q.ptr, ORDER_PARAM, b"0");
            changed |= set_if_changed(host, q.ptr, PRIMARY_PARAM, primary.as_bytes());
        }
        let flags: Vec<(MemberFacts, bool)> =
            ordered.iter().map(|(q, f)| (*f, is_probing(host, q.ptr))).collect();
        for (i, place) in display_places(&flags) {
            let q = &ordered[i].0;
            changed |= set_if_changed(host, q.ptr, ORDER_PARAM, place.to_string().as_bytes());
            changed |= set_if_changed(host, q.ptr, PRIMARY_PARAM, primary.as_bytes());
        }
        if changed {
            host.dq_save_changed(dq_ptr);
        }
        changed
    }

    /// [`refresh_key_host`] for every fleet key in the queue (the queue
    /// coordinator calls it every few seconds, so labels set later, such as
    /// TwinCheck's, move a copy to its place).
    ///
    /// The queue is read once per pass: each fleet's copies come from that
    /// read, and the primaries' raise compares with one list of rivals, read
    /// again only for the copies a fleet's refresh changed. Read per fleet
    /// (and once more per primary), a queue of 1042 items in 148 fleets held
    /// the queue lock nearly all the time: the queue coordinator spun at 100%
    /// CPU and post-processing and the API waited minutes for the lock (6790,
    /// 2026-11-26).
    pub(crate) fn refresh_all_host(host: &QueueSubsysHost, dq_ptr: usize) {
        if !host.opts().dupe_check {
            return;
        }
        let snaps = queue_snaps(host, dq_ptr);
        // each fleet key once, in queue order, with its copies
        let mut keys: Vec<Vec<u8>> = Vec::new();
        let mut copies: std::collections::HashMap<Vec<u8>, Vec<NzbSnap>> = std::collections::HashMap::new();
        for q in &snaps {
            if holds_in_queue(host, q) {
                let key = q.dupe_key.to_ascii_lowercase();
                if !copies.contains_key(&key) {
                    copies.insert(key.clone(), Vec::new());
                    keys.push(key);
                }
            }
        }
        for q in &snaps {
            if let Some(members) = copies.get_mut(&q.dupe_key.to_ascii_lowercase()) {
                if is_member(q, &q.dupe_key) {
                    members.push(q.clone());
                }
            }
        }
        let mut rivals = Some(Rivals::of(host, &snaps));
        drop(snaps);
        for key in keys {
            let members = copies.remove(&key).unwrap_or_default();
            let outcome = refresh_members(host, dq_ptr, &key, &members, &mut rivals);
            if outcome != Refresh::Unchanged {
                let ptrs: Vec<NzbRef> = members.iter().map(|q| q.ptr).collect();
                if let Some(r) = rivals.as_mut() {
                    if outcome == Refresh::Ended {
                        r.forget(&ptrs);
                    } else {
                        r.update(host, &ptrs);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: i32, alive: i32) -> MemberFacts {
        MemberFacts {
            alive,
            kind: crate::donorkind::DonorKind::Unknown as u8,
            ease: 2,
            score: 100 - id,
            id,
            ..Default::default()
        }
    }

    /// F16a: healthy copies first, by ease, twins before alts; weak copies by
    /// their alive share; unverified, locked (never failed over to), then dead
    /// and parked copies last.
    #[test]
    fn failover_order_follows_the_fleet_rules() {
        let mut stored = m(1, 990);
        stored.ease = 1;
        let mut bare = m(2, 980);
        bare.ease = 0;
        let mut twin = m(3, 990);
        twin.ease = 1;
        twin.twin = true;
        twin.kind = crate::donorkind::DonorKind::Twin as u8;
        let weak40 = m(4, 400);
        let mut weak90 = m(5, 900);
        weak90.ease = 3;
        let unverified = m(6, -1);
        let dead = m(7, 0);
        let mut parked = m(8, 970);
        parked.parked = true;
        let mut locked = m(9, 1000);
        locked.locked = true;
        let members = [stored, bare, twin, weak40, weak90, unverified, dead, parked, locked];
        let ids: Vec<i32> = failover_order(&members).into_iter().map(|i| members[i].id).collect();
        assert_eq!(ids, vec![2, 3, 1, 5, 4, 6, 9, 8, 7]);
        assert!(!dead.live() && !parked.live() && unverified.live());
        assert!(locked.live() && !locked.pickable() && unverified.pickable());
    }

    /// F16a (#717 review), pinned: the alive gate first (healthy before
    /// weak, whatever the ease), then F10 ease among healthy copies, twins
    /// before alts at the same ease, weak copies by their share, then
    /// unverified ones, dead last. A copy with any alive article (0.3%)
    /// ranks ahead of an unchecked one: intended, it can lend what it has.
    #[test]
    fn failover_order_pins_f16a() {
        // ids and scores run against the expected order, so no pair is
        // decided by the tie-break (score, then id) instead of its rule
        let mut bare_alt = m(9, 980);
        bare_alt.ease = 0;
        let mut stored_twin = m(8, 990);
        stored_twin.ease = 1;
        stored_twin.twin = true;
        stored_twin.kind = crate::donorkind::DonorKind::Twin as u8;
        let mut stored_alt = m(7, 1000);
        stored_alt.ease = 1;
        let mut encrypted_alt = m(6, 1000);
        encrypted_alt.ease = 3;
        // weak, but the easiest kind: the alive gate comes first
        let mut weak_bare = m(5, 900);
        weak_bare.ease = 0;
        let mut barely_alive = m(4, 3);
        barely_alive.ease = 0;
        let mut unverified_twin = m(3, -1);
        unverified_twin.twin = true;
        unverified_twin.kind = crate::donorkind::DonorKind::Twin as u8;
        let mut unverified = m(2, -1);
        let mut dead = m(1, 0);
        for f in [
            &mut bare_alt,
            &mut stored_twin,
            &mut stored_alt,
            &mut encrypted_alt,
            &mut weak_bare,
            &mut barely_alive,
            &mut unverified_twin,
            &mut unverified,
            &mut dead,
        ] {
            f.score = f.id;
        }
        let members = [
            dead,
            unverified,
            barely_alive,
            encrypted_alt,
            weak_bare,
            stored_alt,
            unverified_twin,
            stored_twin,
            bare_alt,
        ];
        let ids: Vec<i32> = failover_order(&members).into_iter().map(|i| members[i].id).collect();
        assert_eq!(ids, vec![9, 8, 7, 6, 5, 4, 3, 2, 1]);
        let place = |id: i32| ids.iter().position(|&x| x == id).unwrap();
        assert!(place(4) < place(3) && place(4) < place(2), "0.3% alive before unchecked");
        assert!(place(8) < place(7), "twin before alt at the same ease");
        assert!(place(3) < place(2), "unverified twin before unverified alt");
        assert!(place(6) < place(5), "healthy before weak, whatever the ease");
        assert!(place(9) < place(8), "ease before twin among healthy copies");
    }

    /// Ties keep the order copies came (their scores), then their ids.
    #[test]
    fn failover_order_ties_go_by_score() {
        let members = [m(3, 1000), m(1, 1000), m(2, 1000)];
        let ids: Vec<i32> = failover_order(&members).into_iter().map(|i| members[i].id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(position_label(1), "next");
        assert_eq!(position_label(3), "3");
    }

    /// Review: place 1 ("next") is the copy failover takes: never one still
    /// being probed, a locked one or a dead one.
    #[test]
    fn next_is_the_copy_failover_takes() {
        let healthy = m(1, 1000);
        let weak = m(2, 600);
        let mut locked = m(3, 900);
        locked.locked = true;
        let dead = m(4, 0);
        // in failover order: the probing healthy copy first
        let held = [(healthy, true), (weak, false), (locked, false), (dead, false)];
        assert_eq!(display_order(&held), vec![1, 0, 2, 3]);
        let none_ready = [(healthy, true), (locked, false), (dead, false)];
        assert_eq!(display_order(&none_ready), vec![0, 1, 2]);
        // B5: place 1 is the weak copy failover takes; with none ready no
        // copy gets place 1, so a copy still being probed is never "next"
        assert_eq!(display_places(&held), vec![(1, 1), (0, 2), (2, 3), (3, 4)]);
        assert_eq!(display_places(&none_ready), vec![(0, 2), (1, 3), (2, 4)]);
        let probing_only = [(healthy, true), (weak, true)];
        assert!(display_places(&probing_only).iter().all(|&(_, place)| place >= 2));
    }

    /// Review: a fleet ends only with nothing downloading, nothing probed and
    /// no copy failover can pick; dead copies end as failures, the rest as
    /// donors (F3).
    #[test]
    fn a_fleet_ends_only_without_a_live_copy() {
        let mut parked = m(1, 400);
        parked.parked = true;
        let dead = m(2, 0);
        let mut locked = m(3, 1000);
        locked.locked = true;
        let unverified = m(4, -1);
        let left = [(parked, false), (dead, false), (locked, false)];
        assert!(fleet_ended(0, false, &left));
        assert!(!fleet_ended(1, false, &left), "a copy still downloads");
        assert!(!fleet_ended(0, true, &left), "a probe still runs");
        assert!(!fleet_ended(0, false, &[(unverified, false)]), "an unverified copy is tried");
        assert!(!fleet_ended(0, false, &[(locked, true)]), "a copy is still being probed");
        assert!(!fleet_ended(0, false, &[]));
        assert_eq!(ending_of(&dead), Ending::Failure);
        assert_eq!(ending_of(&parked), Ending::Donor);
        assert_eq!(ending_of(&locked), Ending::Donor);
        assert_eq!(ending_of(&unverified), Ending::Donor);
    }

    /// Review nits: a copy leaving its hold gets the user's priority when
    /// they changed it, else its own; the files a hold paused round-trip.
    #[test]
    fn a_released_copy_keeps_the_users_priority() {
        assert_eq!(released_priority(HELD_PRIORITY, Some(50)), 50);
        assert_eq!(released_priority(HELD_PRIORITY, None), 0);
        assert_eq!(released_priority(100, Some(50)), 100);
        assert_eq!(parse_held_files(held_files_param(&[3, 17, 40]).as_bytes()), vec![3, 17, 40]);
        assert!(parse_held_files(b"").is_empty());
    }

    /// F2c (6790, 2026-11-25): a primary released at priority 0 sat QUEUED for
    /// 45 minutes behind items at 47-96. It goes one above the highest queued
    /// item, never to force, and a priority the user set after the raise stays.
    #[test]
    fn the_primary_goes_above_every_queued_priority() {
        assert_eq!(primary_priority(0, None, &[47, 96, 50]), Some(97));
        assert_eq!(primary_priority(0, None, &[]), None, "nothing to beat");
        assert_eq!(primary_priority(100, None, &[47, 96]), None, "already first");
        assert_eq!(primary_priority(96, None, &[96]), Some(97), "ties go by queue order: one above");
        // force items run anyway; the primary stays under force
        assert_eq!(primary_priority(0, None, &[900, 50]), Some(51));
        assert_eq!(primary_priority(0, None, &[899]), Some(899));
        assert_eq!(primary_priority(899, None, &[899]), None);
        // a newer item above the raise: raised again
        assert_eq!(primary_priority(97, Some(97), &[120]), Some(121));
        // the user changed the raised priority: theirs stays
        assert_eq!(primary_priority(10, Some(97), &[120]), None);
        // held again or leaving the fleet: its own priority back, unless the
        // user changed it since the raise
        assert_eq!(unboosted_priority(97, Some(97), Some(0)), 0);
        assert_eq!(unboosted_priority(50, Some(97), Some(0)), 50);
        assert_eq!(unboosted_priority(50, None, None), 50);
    }

    /// Re-review 1: a nested end or refresh of a key already being handled
    /// does nothing.
    #[test]
    fn a_busy_key_is_not_entered_twice() {
        let outer = BusyKey::enter(b"dupes:Busy.Test").expect("free");
        assert!(BusyKey::enter(b"dupes:busy.test").is_none());
        assert!(BusyKey::enter(b"dupes:other.test").is_some());
        drop(outer);
        assert!(BusyKey::enter(b"dupes:busy.test").is_some());
    }

    /// Re-review nit 2: folders compare once resolved, and ignoring case on macOS.
    #[test]
    fn shared_folders_are_found() {
        let dir = std::env::temp_dir().join(format!("rg-same-folder-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Rel")).unwrap();
        let a = dir.join("Rel");
        let b = dir.join("x").join("..").join("Rel");
        std::fs::create_dir_all(dir.join("x")).unwrap();
        assert!(same_folder(a.to_str().unwrap().as_bytes(), b.to_str().unwrap().as_bytes()));
        let slash = format!("{}/", a.to_str().unwrap());
        assert!(same_folder(a.to_str().unwrap().as_bytes(), slash.as_bytes()));
        assert!(!same_folder(b"/x/Rel", b"/x/Other"));
        assert!(!same_folder(b"", b""));
        assert_eq!(same_folder(b"/nowhere/Rel.A", b"/nowhere/rel.a"), cfg!(target_os = "macos"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probing_guard_marks_a_key_while_it_lives() {
        assert!(!probing(b"dupes:guard.test"));
        {
            let _g = ProbingGuard::enter(b"dupes:Guard.Test");
            assert!(probing(b"dupes:guard.test"));
        }
        assert!(!probing(b"dupes:guard.test"));
    }
}
