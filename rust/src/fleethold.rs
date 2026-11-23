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
    /// Whether failover may pick the copy: alive or unverified, never tried.
    pub fn live(&self) -> bool {
        !self.parked && self.alive != 0
    }

    /// healthy, weak, locked, unverified, dead (the user's rule, F16a)
    fn tier(&self) -> u8 {
        if !self.live() {
            4
        } else if self.locked {
            2
        } else if self.alive < 0 {
            3
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
/// encrypted: F10), twins before alts; locked copies after every extractable
/// one, then unverified copies, and dead or parked copies last. Weak copies
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
            .find(|(q, f)| f.live() && !is_probing(host, q.ptr))
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
    pub(crate) fn hold_host(host: &QueueSubsysHost, dq_ptr: usize, q: &NzbSnap, why: &[u8]) {
        if !is_held(host, q.ptr) {
            host.nzb_set_param(q.ptr, HELD_PRIORITY_PARAM, Some(q.priority.to_string().as_bytes()));
        }
        host.nzb_set_param(q.ptr, HELD_PARAM, Some(b"yes"));
        host.nzb_set_int(q.ptr, crate::queuesubsys_host::NZB_INT_PRIORITY, HELD_PRIORITY);
        set_paused(host, dq_ptr, q, true);
        if !why.is_empty() {
            let mut msg = b"Holding ".to_vec();
            msg.extend_from_slice(&q.name);
            msg.extend_from_slice(b" in its fleet: ");
            msg.extend_from_slice(why);
            host.nzb_print_message(q.ptr, crate::queuesubsys_host::MK_INFO, &crate::releasename::log_name(&msg));
        }
        host.nzb_set_int(q.ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
    }

    /// Releases a held copy: it downloads at its own priority again.
    pub(crate) fn release_host(host: &QueueSubsysHost, dq_ptr: usize, q: &NzbSnap, why: &[u8]) {
        let priority = host
            .nzb_get_param(q.ptr, HELD_PRIORITY_PARAM)
            .and_then(|v| std::str::from_utf8(&v).ok()?.trim().parse::<i32>().ok())
            .unwrap_or(0);
        host.nzb_set_param(q.ptr, HELD_PARAM, None);
        host.nzb_set_param(q.ptr, HELD_PRIORITY_PARAM, None);
        host.nzb_set_param(q.ptr, PROBING_PARAM, None);
        host.nzb_set_int(q.ptr, crate::queuesubsys_host::NZB_INT_PRIORITY, priority);
        set_paused(host, dq_ptr, q, false);
        let mut msg = b"Releasing ".to_vec();
        msg.extend_from_slice(&q.name);
        msg.extend_from_slice(b" from its fleet: ");
        msg.extend_from_slice(why);
        host.nzb_print_message(q.ptr, crate::queuesubsys_host::MK_INFO, &crate::releasename::log_name(&msg));
        host.nzb_set_int(q.ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
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
            fail_held_host(host, dq_ptr, nzb);
        }
        host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Health);
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_PARK_DELETE,
            None,
        );
    }

    /// The fleet of `nzb` failed (no live copy left): its held copies go to
    /// history as failures (FAILURE/HEALTH, files and nzb kept), so the group
    /// shows the failure instead of waiting in the queue for ever.
    pub(crate) fn fail_held_host(host: &QueueSubsysHost, dq_ptr: usize, nzb: &NzbSnap) {
        let held: Vec<NzbSnap> = queued_members(host, dq_ptr, &nzb.dupe_key)
            .into_iter()
            .filter(|q| q.ptr != nzb.ptr && is_held(host, q.ptr))
            .collect();
        for q in held {
            let mut msg = b"Every copy of the fleet of ".to_vec();
            msg.extend_from_slice(&q.name);
            msg.extend_from_slice(b" failed: it goes to history with them");
            host.nzb_print_message(q.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
            if host.nzb_get_param(q.ptr, crate::dupecoord::PROJECTED_PARAM).is_none() {
                let alive = host.nzb_get_param(q.ptr, b"DupeAlive").unwrap_or_else(|| b"0".to_vec());
                host.nzb_set_param(q.ptr, crate::dupecoord::PROJECTED_PARAM, Some(&alive));
            }
            host.nzb_set_param(q.ptr, HELD_PARAM, None);
            host.nzb_set_param(q.ptr, PROBING_PARAM, None);
            host.nzb_set_delete_status(q.ptr, DeleteStatus::Health);
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
            host.nzb_set_param(q.ptr, HELD_PARAM, None);
            host.nzb_set_param(q.ptr, PROBING_PARAM, None);
            host.nzb_set_delete_status(q.ptr, DeleteStatus::Dupe);
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
            let first = ordered
                .iter()
                .position(|(q, f)| f.live() && !is_probing(host, q.ptr));
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
        for (pos, (q, _)) in ordered.iter().enumerate() {
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
    /// their alive share; locked, unverified, then dead and parked copies last.
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
        assert_eq!(ids, vec![2, 3, 1, 5, 4, 9, 6, 8, 7]);
        assert!(!dead.live() && !parked.live() && unverified.live());
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
