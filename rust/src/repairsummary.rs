//! How much a job repaired, and how, kept with the job into history.
//!
//! Live PAR forgets its session once a job is done, and a job that needed no
//! repair looks the same as one repaired while it downloaded. This module
//! counts each repair as it happens (live PAR while downloading or as the
//! download ends, par-repair after the download, articles borrowed from
//! duplicates) and keeps the counts in the job's `RepairStats` parameter, so
//! they are saved with the queue, survive a restart and reach the history RPC
//! with the other parameters.
//!
//! Counts noted from repair threads wait in memory and are added to the
//! parameter at each queue checkpoint and when the job reaches history, the
//! way live PAR's time is (`postproc::live_par_time_settle`). At history the
//! parameter also gets the method and the failed-article split, and the job's
//! log gets one summary line.
//!
//! Wording follows AGENTS.md: a twin is the same post uploaded again (proven);
//! an alt is the same video in another container. Articles swapped one for one
//! come from a twin or from a re-RAR alt (`twinfields`); a duplicate without
//! either label is counted as unlabelled. Stream repair matches the video
//! bytes through the payload map and may use a twin or an alt; its borrows are
//! counted as `payload`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::queuesubsys_host::{NzbRef, QueueSubsysHost};

/// The job parameter holding the counts (`key=value` pairs joined by `;`).
pub(crate) const PARAM: &[u8] = b"RepairStats";

/// A donor's label as far as one-for-one article swaps go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Donor {
    Twin,
    Alt,
    Unlabelled,
}

/// Counts for one job. Slices and blocks are par2 slices / recovery blocks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Damaged slices live PAR repaired while downloading or as the download ended
    /// (including slices rebuilt ahead).
    pub live_slices: u64,
    /// Recovery blocks live PAR's repairs used.
    pub live_recovery: u64,
    /// Recovery blocks live PAR had at hand when it repaired (the most seen).
    pub live_available: u64,
    /// Damaged slices par-repair rebuilt after the download.
    pub post_slices: u64,
    /// Recovery blocks par-repair had (the most seen).
    pub post_available: u64,
    /// Bytes live PAR read back from disk (articles RAM no longer held, back-fill).
    pub read_back: u64,
    /// Articles swapped in one for one from a twin, an alt (re-RAR), or an unlabelled copy.
    pub twin: i64,
    pub alt: i64,
    pub unlabelled: i64,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.live_slices = self.live_slices.saturating_add(other.live_slices);
        self.live_recovery = self.live_recovery.saturating_add(other.live_recovery);
        self.live_available = self.live_available.max(other.live_available);
        self.post_slices = self.post_slices.saturating_add(other.post_slices);
        self.post_available = self.post_available.max(other.post_available);
        self.read_back = self.read_back.saturating_add(other.read_back);
        self.twin = self.twin.saturating_add(other.twin);
        self.alt = self.alt.saturating_add(other.alt);
        self.unlabelled = self.unlabelled.saturating_add(other.unlabelled);
    }

    fn is_empty(&self) -> bool {
        *self == Counts::default()
    }

    fn borrowed(&self) -> i64 {
        self.twin.max(0) + self.alt.max(0) + self.unlabelled.max(0)
    }
}

#[derive(Default)]
struct Pending {
    counts: Counts,
    /// Donor labels seen when an article was borrowed, for taking it back later.
    donors: HashMap<i32, Donor>,
}

static PENDING: LazyLock<Mutex<HashMap<i32, Pending>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn with(nzb_id: i32, f: impl FnOnce(&mut Pending)) {
    if nzb_id <= 0 {
        return;
    }
    let mut map = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    f(map.entry(nzb_id).or_default());
}

/// Live PAR repaired `slices` slices of job `nzb_id` using `recovery_used` of the
/// `available` recovery blocks at hand.
pub(crate) fn note_live(nzb_id: i32, slices: usize, recovery_used: usize, available: usize) {
    if slices == 0 && recovery_used == 0 {
        return;
    }
    with(nzb_id, |p| {
        p.counts.live_slices = p.counts.live_slices.saturating_add(slices as u64);
        p.counts.live_recovery = p.counts.live_recovery.saturating_add(recovery_used as u64);
        p.counts.live_available = p.counts.live_available.max(available as u64);
    });
}

/// Par-repair after the download rebuilt `slices` slices with `available` recovery
/// blocks at hand (it uses one recovery block per rebuilt slice).
pub(crate) fn note_post(nzb_id: i32, slices: u64, available: u64) {
    with(nzb_id, |p| {
        p.counts.post_slices = p.counts.post_slices.saturating_add(slices);
        p.counts.post_available = p.counts.post_available.max(available);
    });
}

/// Live PAR's session for job `nzb_id` ended having read `bytes` back from disk.
pub(crate) fn note_read_back(nzb_id: i32, bytes: u64) {
    if bytes == 0 {
        return;
    }
    with(nzb_id, |p| {
        p.counts.read_back = p.counts.read_back.saturating_add(bytes)
    });
}

/// One article of job `nzb_id` was borrowed from duplicate `donor_id`, labelled `donor`.
pub(crate) fn note_borrow(nzb_id: i32, donor_id: i32, donor: Donor) {
    with(nzb_id, |p| {
        p.donors.insert(donor_id, donor);
        bump(&mut p.counts, donor, 1);
    });
}

/// A borrowed article of job `nzb_id` (from `donor_id`) failed its check and no longer counts.
pub(crate) fn note_unborrow(nzb_id: i32, donor_id: i32) {
    with(nzb_id, |p| {
        let donor = p
            .donors
            .get(&donor_id)
            .copied()
            .unwrap_or(Donor::Unlabelled);
        bump(&mut p.counts, donor, -1);
    });
}

fn bump(counts: &mut Counts, donor: Donor, delta: i64) {
    let slot = match donor {
        Donor::Twin => &mut counts.twin,
        Donor::Alt => &mut counts.alt,
        Donor::Unlabelled => &mut counts.unlabelled,
    };
    *slot = slot.saturating_add(delta);
}

/// The label of duplicate `donor` (a queue or history item) for article swaps.
pub(crate) fn donor_label(host: &QueueSubsysHost, donor: NzbRef) -> Donor {
    if donor.is_none() {
        return Donor::Unlabelled;
    }
    let kind = host.nzb_get_param(donor, b"DupeKind");
    let kind = kind.as_deref().map(<[u8]>::trim_ascii);
    if kind.is_some_and(|k| k.starts_with(b"twin"))
        || crate::donorkind::DonorKind::from_param(
            host.nzb_get_param(donor, crate::donorkind::KIND_PARAM)
                .as_deref(),
        ) == crate::donorkind::DonorKind::Twin
    {
        return Donor::Twin;
    }
    let sampled = host.nzb_get_param(donor, b"DupeSampled");
    if kind.is_some_and(|k| k.starts_with(b"alt"))
        || sampled
            .as_deref()
            .map(<[u8]>::trim_ascii)
            .is_some_and(|s| s.starts_with(b"rerar"))
    {
        return Donor::Alt;
    }
    Donor::Unlabelled
}

/// The queue or history item with `id`, under the queue lock `dq_ptr`.
pub(crate) fn find_item(host: &QueueSubsysHost, dq_ptr: usize, id: i32) -> NzbRef {
    let queued = host.queue_find_by_id(dq_ptr, id);
    if !queued.is_none() {
        return queued;
    }
    (0..host.history_len(dq_ptr))
        .map(|i| host.history_at(dq_ptr, i))
        .find(|h| h.kind == crate::queuesubsys_host::HK_NZB && h.id == id)
        .map_or(NzbRef::NONE, |h| h.nzb())
}

/// Drops job `nzb_id`'s unsaved counts (the job left the queue without history, or
/// was merged away).
pub(crate) fn discard(nzb_id: i32) {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&nzb_id);
}

fn take(nzb_id: i32) -> Counts {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(&nzb_id)
        .map(|p| std::mem::take(&mut p.counts))
        .unwrap_or_default()
}

/// The saved record: the counts, and once the job reached history, the outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Record {
    pub counts: Counts,
    /// `clean`, `par2only`, `unrepaired`, or the methods used joined by `+`
    /// (`live`, `par`, `twin`, `alt`, `payload`, `unlabelled`). Empty while queued.
    pub method: String,
    /// Articles stream repair borrowed through the payload map.
    pub payload: i64,
    /// Failed bytes in data files and in par2 files.
    pub failed_data: i64,
    pub failed_par: i64,
    pub repair_sec: i64,
}

pub(crate) fn parse(value: &[u8]) -> Record {
    let mut r = Record::default();
    let text = String::from_utf8_lossy(value);
    for pair in text.split(';') {
        let Some((key, val)) = pair.split_once('=') else {
            continue;
        };
        let (key, val) = (key.trim(), val.trim());
        let int = || val.parse::<i64>().unwrap_or(0);
        let nat = || val.parse::<u64>().unwrap_or(0);
        let triple = || {
            let mut it = val.split('/').map(|v| v.trim().parse::<u64>().unwrap_or(0));
            (
                it.next().unwrap_or(0),
                it.next().unwrap_or(0),
                it.next().unwrap_or(0),
            )
        };
        match key {
            "live" => {
                let (s, u, a) = triple();
                r.counts.live_slices = s;
                r.counts.live_recovery = u;
                r.counts.live_available = a;
            }
            "par" => {
                let (s, _, a) = triple();
                r.counts.post_slices = s;
                r.counts.post_available = a;
            }
            "readback" => r.counts.read_back = nat(),
            "twin" => r.counts.twin = int(),
            "alt" => r.counts.alt = int(),
            "unlabelled" => r.counts.unlabelled = int(),
            "payload" => r.payload = int(),
            "faildata" => r.failed_data = int(),
            "failpar" => r.failed_par = int(),
            "sec" => r.repair_sec = int(),
            "method" => r.method = known_method(val),
            _ => {}
        }
    }
    r
}

/// Only the names this module writes survive a read; anything else is dropped.
fn known_method(val: &str) -> String {
    const OUTCOMES: [&str; 3] = ["clean", "par2only", "unrepaired"];
    const WAYS: [&str; 6] = ["live", "par", "twin", "alt", "payload", "unlabelled"];
    if OUTCOMES.contains(&val) {
        return val.to_string();
    }
    let mut ways: Vec<&str> = Vec::new();
    for way in val.split('+').map(str::trim) {
        if WAYS.contains(&way) && !ways.contains(&way) {
            ways.push(way);
        }
    }
    ways.join("+")
}

pub(crate) fn format(r: &Record) -> String {
    let c = &r.counts;
    let mut out = format!(
        "v=1;live={}/{}/{};par={}/{}/{};readback={};twin={};alt={};unlabelled={}",
        c.live_slices,
        c.live_recovery,
        c.live_available,
        c.post_slices,
        c.post_slices,
        c.post_available,
        c.read_back,
        c.twin,
        c.alt,
        c.unlabelled
    );
    if !r.method.is_empty() {
        out.push_str(&format!(
            ";payload={};faildata={};failpar={};sec={};method={}",
            r.payload, r.failed_data, r.failed_par, r.repair_sec, r.method
        ));
    }
    out
}

/// What the job's repair came to, from its counts and its failed bytes.
pub(crate) fn method(c: &Counts, payload: i64, failed_data: i64, failed_par: i64) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if c.live_slices > 0 {
        parts.push("live");
    }
    if c.post_slices > 0 {
        parts.push("par");
    }
    if c.twin > 0 {
        parts.push("twin");
    }
    if c.alt > 0 {
        parts.push("alt");
    }
    if payload > 0 {
        parts.push("payload");
    }
    if c.unlabelled > 0 {
        parts.push("unlabelled");
    }
    if !parts.is_empty() {
        return parts.join("+");
    }
    if failed_data <= 0 && failed_par <= 0 {
        "clean".into()
    } else if failed_data <= 0 {
        "par2only".into()
    } else {
        "unrepaired".into()
    }
}

fn saved(host: &QueueSubsysHost, nzb_ptr: NzbRef) -> Record {
    host.nzb_get_param(nzb_ptr, PARAM)
        .map_or_else(Record::default, |v| parse(&v))
}

/// Queue checkpoint: adds job `nzb_ptr`'s waiting counts to its saved parameter.
pub(crate) fn checkpoint(host: &QueueSubsysHost, nzb_ptr: NzbRef) {
    let id = host.nzb_snap(nzb_ptr, false).id;
    let pending = take(id);
    if pending.is_empty() {
        return;
    }
    let mut record = saved(host, nzb_ptr);
    record.counts.add(&pending);
    host.nzb_set_param(nzb_ptr, PARAM, Some(format(&record).as_bytes()));
}

/// The job reaches history: its counts are saved with the outcome and logged once.
/// A job leaving without history (`keep` false, or deleted) only drops its counts.
pub(crate) fn settle(host: &QueueSubsysHost, nzb_ptr: NzbRef, keep: bool) {
    let nzb = host.nzb_snap(nzb_ptr, false);
    if !keep || nzb.delete_status != crate::status::DeleteStatus::None {
        discard(nzb.id);
        return;
    }
    let pending = take(nzb.id);
    discard(nzb.id);
    let mut record = saved(host, nzb_ptr);
    record.counts.add(&pending);
    let borrowed = record.counts.borrowed();
    record.payload = (i64::from(nzb.dupe_recovered_articles) - borrowed).max(0);
    record.failed_par = nzb.par_failed_size.max(0);
    record.failed_data = (nzb.failed_size - nzb.par_failed_size).max(0);
    record.repair_sec = i64::from(nzb.repair_sec.max(0));
    record.method = method(
        &record.counts,
        record.payload,
        record.failed_data,
        record.failed_par,
    );
    host.nzb_set_param(nzb_ptr, PARAM, Some(format(&record).as_bytes()));
    let mut line = b"Repair summary for ".to_vec();
    line.extend_from_slice(&nzb.name);
    line.extend_from_slice(b": ");
    line.extend_from_slice(summary_line(&record).as_bytes());
    host.nzb_print_message(nzb_ptr, crate::queuesubsys_host::MK_INFO, &line);
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// "repaired 12 slice(s) live (12 of 37 recovery blocks), 0 after download; ..."
pub(crate) fn summary_line(r: &Record) -> String {
    let c = &r.counts;
    let mut parts = Vec::new();
    match r.method.as_str() {
        "clean" => parts.push("no repair needed (no failed articles)".to_string()),
        "par2only" => parts.push(format!(
            "nothing to repair: failed articles only in par2 files ({})",
            mb(r.failed_par as u64)
        )),
        "unrepaired" => parts.push(format!(
            "not repaired: {} of data files failed",
            mb(r.failed_data as u64)
        )),
        _ => {}
    }
    if c.live_slices > 0 {
        parts.push(format!(
            "repaired {} slice(s) live ({} of {} recovery blocks)",
            c.live_slices, c.live_recovery, c.live_available
        ));
    }
    if c.post_slices > 0 {
        parts.push(format!(
            "repaired {} slice(s) after download ({} of {} recovery blocks)",
            c.post_slices, c.post_slices, c.post_available
        ));
    }
    let borrowed = c.borrowed();
    if borrowed > 0 {
        parts.push(format!(
            "borrowed {borrowed} article(s) one for one (twin {}, alt {}, unlabelled {})",
            c.twin.max(0),
            c.alt.max(0),
            c.unlabelled.max(0)
        ));
    }
    if r.payload > 0 {
        parts.push(format!(
            "{} article(s) matched by video bytes from duplicates",
            r.payload
        ));
    }
    parts.push(format!("{} read back from disk", mb(c.read_back)));
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trips_and_ignores_unknown_keys() {
        let r = Record {
            counts: Counts {
                live_slices: 12,
                live_recovery: 12,
                live_available: 37,
                post_slices: 3,
                post_available: 40,
                read_back: 1 << 20,
                twin: 5,
                alt: 2,
                unlabelled: 1,
            },
            method: "live+par+twin+alt+unlabelled".into(),
            payload: 4,
            failed_data: 100,
            failed_par: 50,
            repair_sec: 7,
        };
        assert_eq!(parse(format(&r).as_bytes()), r);
        let queued = Record {
            counts: r.counts,
            ..Record::default()
        };
        assert_eq!(parse(format(&queued).as_bytes()), queued);
        assert_eq!(
            parse(b"future=1;live=1/2/3;junk;method=<b>live</b>")
                .counts
                .live_available,
            3
        );
        assert_eq!(parse(b"method=<b>live</b>").method, "");
        assert_eq!(parse(b"method=live+<b>+twin+live").method, "live+twin");
        assert_eq!(parse(b"method=par2only").method, "par2only");
    }

    #[test]
    fn method_names_each_way_and_the_cases_without_repair() {
        let none = Counts::default();
        assert_eq!(method(&none, 0, 0, 0), "clean");
        assert_eq!(method(&none, 0, 0, 1024), "par2only");
        assert_eq!(method(&none, 0, 10, 1024), "unrepaired");
        let live = Counts {
            live_slices: 12,
            ..Counts::default()
        };
        assert_eq!(method(&live, 0, 10, 0), "live");
        let mixed = Counts {
            post_slices: 1,
            twin: 3,
            ..Counts::default()
        };
        assert_eq!(method(&mixed, 2, 10, 0), "par+twin+payload");
        // borrowed articles all taken back again: nothing borrowed
        let back = Counts {
            alt: 0,
            ..Counts::default()
        };
        assert_eq!(method(&back, 0, 0, 0), "clean");
    }

    #[test]
    fn pending_counts_keep_the_donor_label_for_a_take_back() {
        let id = 987_654;
        note_borrow(id, 11, Donor::Twin);
        note_borrow(id, 12, Donor::Alt);
        note_borrow(id, 12, Donor::Alt);
        note_unborrow(id, 12);
        note_unborrow(id, 99); // never seen: unlabelled
        note_live(id, 4, 4, 20);
        note_live(id, 2, 2, 18);
        note_post(id, 3, 30);
        note_read_back(id, 5);
        let c = take(id);
        assert_eq!((c.twin, c.alt, c.unlabelled), (1, 1, -1));
        assert_eq!(
            (c.live_slices, c.live_recovery, c.live_available),
            (6, 6, 20)
        );
        assert_eq!((c.post_slices, c.post_available, c.read_back), (3, 30, 5));
        assert!(take(id).is_empty(), "taken counts are not added twice");
        discard(id);
    }

    #[test]
    fn summary_line_says_how_and_how_much() {
        let mut r = Record {
            counts: Counts {
                live_slices: 12,
                live_recovery: 12,
                live_available: 37,
                ..Counts::default()
            },
            ..Record::default()
        };
        r.method = method(&r.counts, 0, 1, 0);
        assert_eq!(
            summary_line(&r),
            "repaired 12 slice(s) live (12 of 37 recovery blocks); 0.0 MB read back from disk"
        );
        let par2 = Record {
            method: "par2only".into(),
            failed_par: 3 << 20,
            ..Record::default()
        };
        assert!(summary_line(&par2)
            .starts_with("nothing to repair: failed articles only in par2 files (3.0 MB)"));
    }
}
