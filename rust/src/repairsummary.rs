//! How much a job repaired, and how, kept with the job into history.
//!
//! Live PAR forgets its session once a job is done, and a job that needed no
//! repair looks the same as one repaired while it downloaded. This module
//! counts each repair as it happens (live PAR while downloading or as the
//! download ends, par-repair after the download, articles borrowed from
//! duplicates, video bytes matched through the payload map) and keeps the
//! counts in the job's `RepairStats` parameter, so they are saved with the
//! queue, survive a restart and reach the history RPC with the other
//! parameters.
//!
//! Counts noted from repair threads wait in memory and are added to the
//! parameter at each queue checkpoint and when the job reaches history, the
//! way live PAR's time is (`postproc::live_par_time_settle`). At history the
//! parameter also gets the method and the failed-article split, and the job's
//! log gets one summary line. Notes arriving after that (live work that ended
//! after history) are dropped until the job returns to the queue.
//!
//! A job that leaves the queue deleted (also one kept in history as a health,
//! dupe or manual delete) keeps no record, like its live PAR time. A merged
//! job's counts move to the job it was merged into; a split leaves them with
//! the original job.
//!
//! Wording follows AGENTS.md. A twin is the exact same post uploaded again,
//! proven by TwinCheck (`DupeKind=twin`) or the fleet (`DonorKind` twin). An
//! alt is the same video in another container (`DupeKind=alt`, or a re-RAR,
//! `DupeSampled=rerar:`). Another container whose video is not verified yet
//! (`container`, or a near-twin `twin:N/M`) and a copy with another video
//! (`other`) keep those labels; a duplicate TwinCheck has not labelled at all
//! is `unlabelled`. Stream repair and borrowed dead volumes match the video
//! bytes through the payload map and may use a twin or an alt; they are
//! counted as `payload`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::queuesubsys_host::{NzbRef, QueueSubsysHost};

/// The job parameter holding the counts (`key=value` pairs joined by `;`).
pub(crate) const PARAM: &[u8] = b"RepairStats";

/// Donor labels kept per job, for taking a borrowed article back after a restart.
const MAX_DONORS: usize = 64;

/// A donor's label for one-for-one article swaps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Donor {
    Twin,
    Alt,
    Container,
    Other,
    Unlabelled,
}

impl Donor {
    fn name(self) -> &'static str {
        match self {
            Donor::Twin => "twin",
            Donor::Alt => "alt",
            Donor::Container => "container",
            Donor::Other => "other",
            Donor::Unlabelled => "unlabelled",
        }
    }

    fn from_name(name: &str) -> Option<Donor> {
        Some(match name {
            "twin" => Donor::Twin,
            "alt" => Donor::Alt,
            "container" => Donor::Container,
            "other" => Donor::Other,
            "unlabelled" => Donor::Unlabelled,
            _ => return None,
        })
    }
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
    /// Articles swapped in one for one, by the donor's label.
    pub twin: i64,
    pub alt: i64,
    pub container: i64,
    pub other: i64,
    pub unlabelled: i64,
    /// Stream repair through the payload map: articles, bytes and holes filled.
    pub payload_articles: u64,
    pub payload_bytes: u64,
    pub payload_holes: u64,
    /// Dead volumes rebuilt from a duplicate's video bytes (their bytes are in
    /// `payload_bytes`).
    pub volumes: u64,
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
        self.container = self.container.saturating_add(other.container);
        self.other = self.other.saturating_add(other.other);
        self.unlabelled = self.unlabelled.saturating_add(other.unlabelled);
        self.payload_articles = self.payload_articles.saturating_add(other.payload_articles);
        self.payload_bytes = self.payload_bytes.saturating_add(other.payload_bytes);
        self.payload_holes = self.payload_holes.saturating_add(other.payload_holes);
        self.volumes = self.volumes.saturating_add(other.volumes);
    }

    fn is_empty(&self) -> bool {
        *self == Counts::default()
    }

    pub(crate) fn borrowed(&self) -> i64 {
        self.twin.max(0)
            + self.alt.max(0)
            + self.container.max(0)
            + self.other.max(0)
            + self.unlabelled.max(0)
    }

    fn payload(&self) -> bool {
        self.payload_articles > 0 || self.payload_bytes > 0 || self.payload_holes > 0
    }

    fn slot(&mut self, donor: Donor) -> &mut i64 {
        match donor {
            Donor::Twin => &mut self.twin,
            Donor::Alt => &mut self.alt,
            Donor::Container => &mut self.container,
            Donor::Other => &mut self.other,
            Donor::Unlabelled => &mut self.unlabelled,
        }
    }
}

#[derive(Default)]
struct Pending {
    counts: Counts,
    /// Donor labels seen when an article was borrowed (kept across checkpoints).
    donors: HashMap<i32, Donor>,
    /// Borrowed articles taken back whose donor label was not at hand: resolved
    /// against the saved donor list at the next checkpoint.
    unresolved: Vec<i32>,
}

#[derive(Default)]
struct State {
    jobs: HashMap<i32, Pending>,
    /// Jobs already settled into history: later notes are dropped.
    closed: std::collections::HashSet<i32>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn with(nzb_id: i32, f: impl FnOnce(&mut Pending)) {
    if nzb_id <= 0 {
        return;
    }
    let mut st = state();
    if st.closed.contains(&nzb_id) {
        return;
    }
    f(st.jobs.entry(nzb_id).or_default());
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

/// Live PAR's session for job `nzb_id` read `bytes` back from disk.
pub(crate) fn note_read_back(nzb_id: i32, bytes: u64) {
    if bytes == 0 {
        return;
    }
    with(nzb_id, |p| {
        p.counts.read_back = p.counts.read_back.saturating_add(bytes)
    });
}

/// Stream repair filled `articles` articles, `bytes` bytes and `holes` holes of job
/// `nzb_id` with video bytes matched through the payload map.
pub(crate) fn note_payload(nzb_id: i32, articles: u64, bytes: u64, holes: u64) {
    if articles == 0 && bytes == 0 && holes == 0 {
        return;
    }
    with(nzb_id, |p| {
        p.counts.payload_articles = p.counts.payload_articles.saturating_add(articles);
        p.counts.payload_bytes = p.counts.payload_bytes.saturating_add(bytes);
        p.counts.payload_holes = p.counts.payload_holes.saturating_add(holes);
    });
}

/// A dead volume of job `nzb_id` was rebuilt from `bytes` of a duplicate's video bytes.
pub(crate) fn note_dead_volume(nzb_id: i32, bytes: u64) {
    with(nzb_id, |p| {
        p.counts.volumes = p.counts.volumes.saturating_add(1);
        p.counts.payload_bytes = p.counts.payload_bytes.saturating_add(bytes);
    });
}

/// The label of duplicate `donor_id` cached for job `nzb_id`, if one was noted.
pub(crate) fn cached_label(nzb_id: i32, donor_id: i32) -> Option<Donor> {
    state()
        .jobs
        .get(&nzb_id)
        .and_then(|p| p.donors.get(&donor_id).copied())
}

/// One article of job `nzb_id` was borrowed from duplicate `donor_id`, labelled `donor`.
pub(crate) fn note_borrow(nzb_id: i32, donor_id: i32, donor: Donor) {
    with(nzb_id, |p| {
        p.donors.entry(donor_id).or_insert(donor);
        *p.counts.slot(donor) += 1;
    });
}

/// A borrowed article of job `nzb_id` (from `donor_id`) failed its check and no longer counts.
pub(crate) fn note_unborrow(nzb_id: i32, donor_id: i32) {
    with(nzb_id, |p| match p.donors.get(&donor_id).copied() {
        Some(donor) => *p.counts.slot(donor) -= 1,
        None => p.unresolved.push(donor_id),
    });
}

/// The label of duplicate `donor` (a queue or history item) for article swaps.
pub(crate) fn donor_label(host: &QueueSubsysHost, donor: NzbRef) -> Donor {
    if donor.is_none() {
        return Donor::Unlabelled;
    }
    let kind = host.nzb_get_param(donor, b"DupeKind");
    let kind = kind.as_deref().map(<[u8]>::trim_ascii).unwrap_or_default();
    // only a full twin is proven; a near-twin (twin:N/M) is another container
    if kind == b"twin"
        || crate::donorkind::DonorKind::from_param(
            host.nzb_get_param(donor, crate::donorkind::KIND_PARAM)
                .as_deref(),
        ) == crate::donorkind::DonorKind::Twin
    {
        return Donor::Twin;
    }
    let sampled = host.nzb_get_param(donor, b"DupeSampled");
    if kind == b"alt"
        || sampled
            .as_deref()
            .map(<[u8]>::trim_ascii)
            .is_some_and(|s| s.starts_with(b"rerar"))
    {
        return Donor::Alt;
    }
    if kind == b"container" || kind.starts_with(b"twin:") {
        return Donor::Container;
    }
    if kind == b"other" {
        return Donor::Other;
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

/// Drops job `nzb_id`'s unsaved counts (the job left the queue without history, was
/// merged away, or returned to the queue from history: it counts again from here).
pub(crate) fn discard(nzb_id: i32) {
    let mut st = state();
    st.jobs.remove(&nzb_id);
    st.closed.remove(&nzb_id);
}

/// Takes job `nzb_id`'s waiting counts and unresolved take-backs, keeping its donor
/// labels; `close` also drops the labels and refuses later notes.
fn take(nzb_id: i32, close: bool) -> (Counts, HashMap<i32, Donor>, Vec<i32>) {
    let mut st = state();
    if close {
        st.closed.insert(nzb_id);
        return st
            .jobs
            .remove(&nzb_id)
            .map(|p| (p.counts, p.donors, p.unresolved))
            .unwrap_or_default();
    }
    st.jobs
        .get_mut(&nzb_id)
        .map(|p| {
            (
                std::mem::take(&mut p.counts),
                p.donors.clone(),
                std::mem::take(&mut p.unresolved),
            )
        })
        .unwrap_or_default()
}

/// The saved record: the counts, and once the job reached history, the outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Record {
    pub counts: Counts,
    /// Donor labels of the borrowed articles, by duplicate id.
    pub donors: Vec<(i32, Donor)>,
    /// `clean`, `par2only`, `unrepaired`, or the ways used joined by `+` (`live`,
    /// `par`, `twin`, `alt`, `container`, `other`, `unlabelled`, `payload`). Empty
    /// while queued.
    pub method: String,
    /// Par-check of the job failed: whatever was repaired, it was not enough.
    pub failed: bool,
    /// Failed bytes in data files and in par2 files.
    pub failed_data: i64,
    pub failed_par: i64,
    pub repair_sec: i64,
}

impl Record {
    /// Adds waiting counts, labels and take-backs (resolved against the labels).
    fn absorb(&mut self, counts: &Counts, donors: &HashMap<i32, Donor>, unresolved: &[i32]) {
        self.counts.add(counts);
        for (&id, &donor) in donors {
            if !self.donors.iter().any(|&(d, _)| d == id) && self.donors.len() < MAX_DONORS {
                self.donors.push((id, donor));
            }
        }
        for id in unresolved {
            let donor = self
                .donors
                .iter()
                .find(|&&(d, _)| d == *id)
                .map_or(Donor::Unlabelled, |&(_, l)| l);
            *self.counts.slot(donor) -= 1;
        }
    }
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
            "container" => r.counts.container = int(),
            "other" => r.counts.other = int(),
            "unlabelled" => r.counts.unlabelled = int(),
            "payload" => {
                let (a, b, h) = triple();
                r.counts.payload_articles = a;
                r.counts.payload_bytes = b;
                r.counts.payload_holes = h;
            }
            "volumes" => r.counts.volumes = nat(),
            "donors" => {
                r.donors = val
                    .split(',')
                    .filter_map(|e| {
                        let (id, l) = e.split_once(':')?;
                        Some((id.trim().parse().ok()?, Donor::from_name(l.trim())?))
                    })
                    .take(MAX_DONORS)
                    .collect();
            }
            "faildata" => r.failed_data = int(),
            "failpar" => r.failed_par = int(),
            "sec" => r.repair_sec = int(),
            "result" => r.failed = val == "failed",
            "method" => r.method = known_method(val),
            _ => {}
        }
    }
    r
}

/// Only the names this module writes survive a read; anything else is dropped.
fn known_method(val: &str) -> String {
    const OUTCOMES: [&str; 3] = ["clean", "par2only", "unrepaired"];
    const WAYS: [&str; 8] = [
        "live",
        "par",
        "twin",
        "alt",
        "container",
        "other",
        "unlabelled",
        "payload",
    ];
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
        "v=2;live={}/{}/{};par={}/{}/{};readback={};twin={};alt={};container={};other={};unlabelled={};payload={}/{}/{};volumes={}",
        c.live_slices,
        c.live_recovery,
        c.live_available,
        c.post_slices,
        c.post_slices,
        c.post_available,
        c.read_back,
        c.twin,
        c.alt,
        c.container,
        c.other,
        c.unlabelled,
        c.payload_articles,
        c.payload_bytes,
        c.payload_holes,
        c.volumes
    );
    if !r.donors.is_empty() {
        out.push_str(";donors=");
        let list: Vec<String> = r
            .donors
            .iter()
            .map(|(id, l)| format!("{id}:{}", l.name()))
            .collect();
        out.push_str(&list.join(","));
    }
    if !r.method.is_empty() {
        out.push_str(&format!(
            ";faildata={};failpar={};sec={};result={};method={}",
            r.failed_data,
            r.failed_par,
            r.repair_sec,
            if r.failed { "failed" } else { "ok" },
            r.method
        ));
    }
    out
}

/// What the job's repair came to, from its counts and its failed bytes.
pub(crate) fn method(c: &Counts, failed_data: i64, failed_par: i64) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if c.live_slices > 0 {
        parts.push("live");
    }
    if c.post_slices > 0 {
        parts.push("par");
    }
    for (n, way) in [
        (c.twin, "twin"),
        (c.alt, "alt"),
        (c.container, "container"),
        (c.other, "other"),
        (c.unlabelled, "unlabelled"),
    ] {
        if n > 0 {
            parts.push(way);
        }
    }
    if c.payload() || c.volumes > 0 {
        parts.push("payload");
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
    let (counts, donors, unresolved) = take(id, false);
    if counts.is_empty() && unresolved.is_empty() {
        let mut record = saved(host, nzb_ptr);
        // new labels alone are saved only when some are missing
        if donors
            .keys()
            .all(|id| record.donors.iter().any(|&(d, _)| d == *id))
        {
            return;
        }
        record.absorb(&counts, &donors, &unresolved);
        host.nzb_set_param(nzb_ptr, PARAM, Some(format(&record).as_bytes()));
        return;
    }
    let mut record = saved(host, nzb_ptr);
    record.absorb(&counts, &donors, &unresolved);
    host.nzb_set_param(nzb_ptr, PARAM, Some(format(&record).as_bytes()));
}

/// Merge: job `src_ptr`'s saved and waiting counts move to job `dest_ptr`, with its files.
pub(crate) fn merge(host: &QueueSubsysHost, dest_ptr: NzbRef, src_ptr: NzbRef) {
    let src_id = host.nzb_snap(src_ptr, false).id;
    let (counts, donors, unresolved) = take(src_id, true);
    discard(src_id);
    let mut src = saved(host, src_ptr);
    src.absorb(&counts, &donors, &unresolved);
    if src.counts.is_empty() && src.donors.is_empty() {
        return;
    }
    let mut dest = saved(host, dest_ptr);
    let src_donors: HashMap<i32, Donor> = src.donors.iter().copied().collect();
    dest.absorb(&src.counts, &src_donors, &[]);
    host.nzb_set_param(dest_ptr, PARAM, Some(format(&dest).as_bytes()));
}

/// The job reaches history: its counts are saved with the outcome and logged once.
/// A job leaving without history (`keep` false, or deleted) only drops its counts.
/// `read_back` is what live PAR's still-open session read back from disk.
pub(crate) fn settle(host: &QueueSubsysHost, nzb_ptr: NzbRef, keep: bool, read_back: u64) {
    let nzb = host.nzb_snap(nzb_ptr, false);
    let (counts, donors, unresolved) = take(nzb.id, true);
    if !keep || nzb.delete_status != crate::status::DeleteStatus::None {
        return;
    }
    let mut record = saved(host, nzb_ptr);
    record.absorb(&counts, &donors, &unresolved);
    record.counts.read_back = record.counts.read_back.saturating_add(read_back);
    record.failed_par = nzb.par_failed_size.max(0);
    record.failed_data = (nzb.failed_size - nzb.par_failed_size).max(0);
    record.repair_sec = i64::from(nzb.repair_sec.max(0));
    record.failed = nzb.par_status == crate::status::NzbParStatus::Failure;
    record.method = method(&record.counts, record.failed_data, record.failed_par);
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

/// "repaired 12 slice(s) live (12 of 37 recovery blocks); ..."
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
            "borrowed {borrowed} article(s) one for one (twin {}, alt {}, container {}, other {}, unlabelled {})",
            c.twin.max(0),
            c.alt.max(0),
            c.container.max(0),
            c.other.max(0),
            c.unlabelled.max(0)
        ));
    }
    if c.payload() || c.volumes > 0 {
        parts.push(format!(
            "{} of video bytes from a twin or alt ({} article(s), {} hole(s), {} dead volume(s))",
            mb(c.payload_bytes),
            c.payload_articles,
            c.payload_holes,
            c.volumes
        ));
    }
    if c.read_back > 0 {
        parts.push(format!("{} read back from disk", mb(c.read_back)));
    }
    if r.failed {
        parts.push("par-check failed".to_string());
    }
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
                container: 1,
                other: 1,
                unlabelled: 1,
                payload_articles: 4,
                payload_bytes: 3 << 20,
                payload_holes: 2,
                volumes: 1,
            },
            donors: vec![(11, Donor::Twin), (12, Donor::Alt)],
            method: "live+par+twin+alt+container+other+unlabelled+payload".into(),
            failed: true,
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
        assert_eq!(
            parse(b"donors=1:twin,x:alt,2:bogus,3:other").donors,
            vec![(1, Donor::Twin), (3, Donor::Other)]
        );
    }

    #[test]
    fn method_names_each_way_and_the_cases_without_repair() {
        let none = Counts::default();
        assert_eq!(method(&none, 0, 0), "clean");
        assert_eq!(method(&none, 0, 1024), "par2only");
        assert_eq!(method(&none, 10, 1024), "unrepaired");
        let live = Counts {
            live_slices: 12,
            ..Counts::default()
        };
        assert_eq!(method(&live, 10, 0), "live");
        let mixed = Counts {
            post_slices: 1,
            twin: 3,
            payload_bytes: 2,
            ..Counts::default()
        };
        assert_eq!(method(&mixed, 10, 0), "par+twin+payload");
        // live stream repair fills holes with bytes only, no articles
        let holes = Counts {
            payload_holes: 3,
            payload_bytes: 4096,
            ..Counts::default()
        };
        assert_eq!(method(&holes, 10, 0), "payload");
        let volume = Counts {
            volumes: 1,
            ..Counts::default()
        };
        assert_eq!(method(&volume, 10, 0), "payload");
        // borrowed articles all taken back again: nothing borrowed
        let back = Counts {
            alt: 0,
            ..Counts::default()
        };
        assert_eq!(method(&back, 0, 0), "clean");
    }

    #[test]
    fn pending_counts_keep_the_donor_label_for_a_take_back() {
        let id = 987_654;
        note_borrow(id, 11, Donor::Twin);
        note_borrow(id, 12, Donor::Alt);
        note_borrow(id, 12, Donor::Alt);
        note_unborrow(id, 12);
        note_unborrow(id, 99); // label not at hand: resolved at the checkpoint
        note_live(id, 4, 4, 20);
        note_live(id, 2, 2, 18);
        note_post(id, 3, 30);
        note_read_back(id, 5);
        note_payload(id, 1, 100, 1);
        note_dead_volume(id, 50);
        assert_eq!(cached_label(id, 12), Some(Donor::Alt));
        let (c, donors, unresolved) = take(id, false);
        assert_eq!((c.twin, c.alt, c.unlabelled), (1, 1, 0));
        assert_eq!(unresolved, vec![99]);
        assert_eq!(
            (c.live_slices, c.live_recovery, c.live_available),
            (6, 6, 20)
        );
        assert_eq!((c.post_slices, c.post_available, c.read_back), (3, 30, 5));
        assert_eq!(
            (
                c.payload_articles,
                c.payload_bytes,
                c.payload_holes,
                c.volumes
            ),
            (1, 150, 1, 1)
        );
        assert_eq!(donors.len(), 2);
        let (again, kept, none) = take(id, false);
        assert!(
            again.is_empty() && none.is_empty(),
            "taken counts are not added twice"
        );
        assert_eq!(kept.len(), 2, "labels stay for later take-backs");
        discard(id);
    }

    #[test]
    fn a_take_back_after_a_restart_uses_the_saved_label() {
        // borrowed from twin 11 before the restart, saved at a checkpoint
        let mut saved = Record::default();
        let mut before = HashMap::new();
        before.insert(11, Donor::Twin);
        saved.absorb(
            &Counts {
                twin: 2,
                ..Counts::default()
            },
            &before,
            &[],
        );
        let saved = parse(format(&saved).as_bytes());
        // after the restart nothing is cached: the take-back waits for the checkpoint
        let id = 987_655;
        note_unborrow(id, 11);
        let (c, donors, unresolved) = take(id, false);
        let mut record = saved;
        record.absorb(&c, &donors, &unresolved);
        assert_eq!((record.counts.twin, record.counts.unlabelled), (1, 0));
        discard(id);
    }

    #[test]
    fn a_settled_job_takes_no_more_notes_until_it_returns() {
        let id = 987_656;
        note_live(id, 1, 1, 1);
        let (c, _, _) = take(id, true);
        assert_eq!(c.live_slices, 1);
        note_read_back(id, 10); // live work ending after history
        assert!(state().jobs.get(&id).is_none(), "no entry left behind");
        discard(id); // back in the queue
        note_read_back(id, 10);
        assert_eq!(take(id, true).0.read_back, 10);
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
        r.method = method(&r.counts, 1, 0);
        assert_eq!(
            summary_line(&r),
            "repaired 12 slice(s) live (12 of 37 recovery blocks)"
        );
        r.counts.read_back = 1 << 20;
        r.failed = true;
        assert_eq!(
            summary_line(&r),
            "repaired 12 slice(s) live (12 of 37 recovery blocks); 1.0 MB read back from disk; par-check failed"
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
