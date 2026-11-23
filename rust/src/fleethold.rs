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
        let mut out = Vec::new();
        for i in 0..host.queue_len(dq_ptr) {
            let qp = host.queue_at(dq_ptr, i);
            if qp.is_none() {
                continue;
            }
            let q = host.nzb_snap(qp, false);
            if q.kind == crate::queuesubsys_host::NK_NZB
                && !q.deleting
                && q.delete_status == DeleteStatus::None
                && q.dupe_key.eq_ignore_ascii_case(key)
            {
                out.push(q);
            }
        }
        out
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
        let held: Vec<NzbSnap> = queued_members(host, dq_ptr, key)
            .into_iter()
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

    /// Holds a queued copy: paused, at the lowest priority, marked [`HELD_PARAM`].
    /// A copy already in the queue records the files the hold paused
    /// ([`HELD_FILES_PARAM`]), so a release resumes only those.
    pub(crate) fn hold_host(host: &QueueSubsysHost, dq_ptr: usize, q: &NzbSnap, why: &[u8]) {
        if !is_held(host, q.ptr) {
            host.nzb_set_param(q.ptr, HELD_PRIORITY_PARAM, Some(q.priority.to_string().as_bytes()));
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
        let stored = host
            .nzb_get_param(ptr, HELD_PRIORITY_PARAM)
            .and_then(|v| std::str::from_utf8(&v).ok()?.trim().parse::<i32>().ok());
        if stored.is_some() || is_held(host, ptr) {
            host.nzb_set_int(ptr, crate::queuesubsys_host::NZB_INT_PRIORITY, released_priority(q.priority, stored));
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
        let held: Vec<(NzbSnap, MemberFacts)> = held_in_order(host, dq_ptr, key, exclude)
            .into_iter()
            .filter(|(q, _)| !is_probing(host, q.ptr))
            .collect();
        for (q, f) in held {
            let ending = ending_of(&f);
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
            // parked: its files and nzb stay
            host.dq_edit_entry(dq_ptr, q.id, crate::queuesubsys_host::EA_GROUP_PARK_DELETE, None);
        }
    }

    /// The primary of a fleet succeeded: every other queued copy goes to
    /// history as DELETED/DUPE (its nzb kept), whatever the HealthCheck mode.
    pub(crate) fn fleet_succeeded_host(host: &QueueSubsysHost, dq_ptr: usize, nzb: &NzbSnap) {
        if !holds_in_queue(host, nzb) {
            return;
        }
        let others: Vec<NzbSnap> = queued_members(host, dq_ptr, &nzb.dupe_key)
            .into_iter()
            .filter(|q| q.ptr != nzb.ptr && !q.has_post_info)
            .collect();
        for q in others {
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
            let shares_folder = !q.dest_dir.is_empty()
                && (q.dest_dir == nzb.dest_dir || q.dest_dir == nzb.final_dir);
            let action = if shares_folder {
                crate::queuesubsys_host::EA_GROUP_PARK_DELETE
            } else {
                crate::queuesubsys_host::EA_GROUP_DUPE_DELETE
            };
            host.dq_edit_entry(dq_ptr, q.id, action, None);
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
        if members.is_empty() {
            return false;
        }
        let probing_now = probing(key);
        let mut changed = false;
        // a copy left probing by a probe that no longer runs (a restart)
        if !probing_now {
            for q in &members {
                if is_probing(host, q.ptr) {
                    host.nzb_set_param(q.ptr, PROBING_PARAM, None);
                    changed = true;
                }
            }
        }
        let mut active: Vec<&NzbSnap> = members.iter().filter(|q| !is_held(host, q.ptr)).collect();
        active.sort_by_key(|q| std::cmp::Reverse(q.dupe_score));
        let mut ordered = held_in_order(host, dq_ptr, key, NzbRef::NONE);
        if active.is_empty() && !probing_now {
            // never a locked copy: failover refuses it
            let first = ordered
                .iter()
                .position(|(q, f)| f.pickable() && !is_probing(host, q.ptr));
            let flags: Vec<(MemberFacts, bool)> =
                ordered.iter().map(|(q, f)| (*f, is_probing(host, q.ptr))).collect();
            if first.is_none() && fleet_ended(0, probing_now, &flags) {
                end_fleet_host(host, dq_ptr, key, NzbRef::NONE);
                host.dq_save_changed(dq_ptr);
                return true;
            }
            if let Some(pos) = first {
                let (q, _) = ordered.remove(pos);
                release_host(host, dq_ptr, &q, b"no copy of its fleet downloads");
                changed = true;
                let id = q.id;
                let snap = members.iter().find(|m| m.id == id).cloned();
                if let Some(snap) = snap {
                    return finish_refresh(host, dq_ptr, &[&snap], &ordered) || changed;
                }
            }
        }
        finish_refresh(host, dq_ptr, &active, &ordered) || changed
    }

    fn finish_refresh(
        host: &QueueSubsysHost,
        dq_ptr: usize,
        active: &[&NzbSnap],
        ordered: &[(NzbSnap, MemberFacts)],
    ) -> bool {
        let primary = active.first().map_or(0, |q| q.id).to_string();
        let mut changed = false;
        for q in active {
            changed |= set_if_changed(host, q.ptr, ORDER_PARAM, b"0");
            changed |= set_if_changed(host, q.ptr, PRIMARY_PARAM, primary.as_bytes());
        }
        let flags: Vec<(MemberFacts, bool)> =
            ordered.iter().map(|(q, f)| (*f, is_probing(host, q.ptr))).collect();
        let shown: Vec<&(NzbSnap, MemberFacts)> = display_order(&flags).into_iter().map(|i| &ordered[i]).collect();
        for (pos, (q, _)) in shown.into_iter().enumerate() {
            changed |= set_if_changed(host, q.ptr, ORDER_PARAM, (pos + 1).to_string().as_bytes());
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
    pub(crate) fn refresh_all_host(host: &QueueSubsysHost, dq_ptr: usize) {
        if !host.opts().dupe_check {
            return;
        }
        let mut keys: Vec<Vec<u8>> = Vec::new();
        for i in 0..host.queue_len(dq_ptr) {
            let qp = host.queue_at(dq_ptr, i);
            if qp.is_none() {
                continue;
            }
            let q = host.nzb_snap(qp, false);
            if holds_in_queue(host, &q) {
                let key = q.dupe_key.to_ascii_lowercase();
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        for key in keys {
            refresh_key_host(host, dq_ptr, &key);
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
