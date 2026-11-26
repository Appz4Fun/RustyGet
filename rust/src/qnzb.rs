//! A queued collection's numeric and status fields, which were C++ `NzbInfo`
//! members, in one Rust allocation with C layout.
//!
//! The C++ `NzbInfo` keeps a pointer to it and reads and writes the fields in
//! place (`m_s->field`), under the download-queue lock as before, so its code
//! and the host operations keep their behaviour while the data moves to Rust.
//! The collection's strings live next to them, behind a mutex, reached through
//! `nzbget_rs_nzb_text` / `nzbget_rs_nzb_set_text`. Collections still live in
//! C++ for now.

#![allow(non_snake_case)]

use crate::jobthread::JobHandle;
use crate::options::OptStr;
use crate::queuesubsys_host::{FileRef, NzbRef, PostRef};
use crate::status::{DeleteStatus, ScriptStatus};
use std::collections::BTreeSet;
use std::ffi::{c_char, c_int, CStr, CString};
use std::sync::Mutex;

/// Shared by pointer between queue modules, hence `#[repr(C)]`.
#[repr(C)]
pub struct NzbScalars {
    pub id: i32,
    pub kind: i32,
    pub fileCount: i32,
    pub parkedFileCount: i32,
    pub size: i64,
    pub remainingSize: i64,
    pub pausedFileCount: i32,
    pub pausedSize: i64,
    pub remainingParCount: i32,
    pub activeDownloads: i32,
    pub successSize: i64,
    pub failedSize: i64,
    pub currentSuccessSize: i64,
    pub currentFailedSize: i64,
    pub parSize: i64,
    pub parSuccessSize: i64,
    pub parFailedSize: i64,
    pub parCurrentSuccessSize: i64,
    pub parCurrentFailedSize: i64,
    pub totalArticles: i32,
    pub successArticles: i32,
    pub failedArticles: i32,
    pub currentSuccessArticles: i32,
    pub currentFailedArticles: i32,
    pub dupeRecoveredArticles: i32,
    pub dupeFailedRun: i32,
    pub dupeRecoveredBytes: i64,
    pub dupeRecoveredHoles: i32,
    pub dupeAttemptedArticles: i32,
    pub dupeUnsourcedArticles: i32,
    pub dupeFailoverChecked: i32,
    pub deadPickProbed: bool,
    pub dupeParDeferState: i32,
    pub dupeNoParNoted: bool,
    pub minTime: i64,
    pub maxTime: i64,
    pub priority: i32,
    pub extraPriority: i32,
    pub directRenameStatus: i32,
    pub parRenameStatus: i32,
    pub rarRenameStatus: i32,
    pub parStatus: i32,
    pub directUnpackStatus: i32,
    pub unpackStatus: i32,
    pub cleanupStatus: i32,
    pub moveStatus: i32,
    pub postUnpackRenamingStatus: i32,
    pub postDownloadRenamingStatus: i32,
    pub deleteStatus: i32,
    pub markStatus: i32,
    pub urlStatus: i32,
    pub extraParBlocks: i32,
    pub addUrlPaused: bool,
    pub deletePaused: bool,
    pub manyDupeFiles: bool,
    pub deleting: bool,
    pub parking: bool,
    pub avoidHistory: bool,
    pub healthPaused: bool,
    pub cleanupDisk: bool,
    pub unpackCleanedUpDisk: bool,
    pub dupeScore: i32,
    pub dupeMode: i32,
    pub dupeHint: i32,
    pub fullContentHash: u32,
    pub filteredContentHash: u32,
    pub idMessageGen: i32,
    pub parRepaired: bool,
    pub unpackTried: bool,
    pub lastUnpackStatus: i32,
    pub downloadedSize: i64,
    pub downloadStartTime: i64,
    pub downloadStartSec: i32,
    pub downloadSec: i32,
    pub postTotalSec: i32,
    pub parSec: i32,
    pub repairSec: i32,
    pub unpackSec: i32,
    pub liveUnpackSec: i32,
    pub liveParSec: i32,
    pub reprocess: bool,
    pub changed: bool,
    pub queueScriptTime: i64,
    pub parFull: bool,
    pub cachedMessageCount: i32,
    pub feedId: i32,
    pub allFirst: bool,
    pub waitingPar: bool,
    pub loadingPar: bool,
    pub desiredServerId: i32,
    pub skipScriptProcessing: bool,
    pub skipDiskWrite: bool,
    pub autoCategory: bool,
}

impl Default for NzbScalars {
    fn default() -> Self {
        Self {
            id: 0,
            kind: 0,
            fileCount: 0,
            parkedFileCount: 0,
            size: 0,
            remainingSize: 0,
            pausedFileCount: 0,
            pausedSize: 0,
            remainingParCount: 0,
            activeDownloads: 0,
            successSize: 0,
            failedSize: 0,
            currentSuccessSize: 0,
            currentFailedSize: 0,
            parSize: 0,
            parSuccessSize: 0,
            parFailedSize: 0,
            parCurrentSuccessSize: 0,
            parCurrentFailedSize: 0,
            totalArticles: 0,
            successArticles: 0,
            failedArticles: 0,
            currentSuccessArticles: 0,
            currentFailedArticles: 0,
            dupeRecoveredArticles: 0,
            dupeFailedRun: 0,
            dupeRecoveredBytes: 0,
            dupeRecoveredHoles: 0,
            dupeAttemptedArticles: 0,
            dupeUnsourcedArticles: 0,
            dupeFailoverChecked: -1,
            deadPickProbed: false,
            dupeParDeferState: 0,
            dupeNoParNoted: false,
            minTime: 0,
            maxTime: 0,
            priority: 0,
            extraPriority: 0,
            directRenameStatus: 0,
            parRenameStatus: 0,
            rarRenameStatus: 0,
            parStatus: 0,
            directUnpackStatus: 0,
            unpackStatus: 0,
            cleanupStatus: 0,
            moveStatus: 0,
            postUnpackRenamingStatus: 0,
            postDownloadRenamingStatus: 0,
            deleteStatus: 0,
            markStatus: 0,
            urlStatus: 0,
            extraParBlocks: 0,
            addUrlPaused: false,
            deletePaused: false,
            manyDupeFiles: false,
            deleting: false,
            parking: false,
            avoidHistory: false,
            healthPaused: false,
            cleanupDisk: false,
            unpackCleanedUpDisk: false,
            dupeScore: 0,
            dupeMode: 0,
            dupeHint: 0,
            fullContentHash: 0,
            filteredContentHash: 0,
            idMessageGen: 0,
            parRepaired: false,
            unpackTried: false,
            lastUnpackStatus: 0,
            downloadedSize: 0,
            downloadStartTime: 0,
            downloadStartSec: 0,
            downloadSec: 0,
            postTotalSec: 0,
            parSec: 0,
            repairSec: 0,
            unpackSec: 0,
            liveUnpackSec: 0,
            liveParSec: 0,
            reprocess: false,
            changed: false,
            queueScriptTime: 0,
            parFull: false,
            cachedMessageCount: 0,
            feedId: 0,
            allFirst: false,
            waitingPar: false,
            loadingPar: false,
            desiredServerId: 0,
            skipScriptProcessing: false,
            skipDiskWrite: false,
            autoCategory: false,
        }
    }
}

/// Text fields, by id: 0 url, 1 filename, 2 name, 3 destination dir, 4 final
/// dir, 5 category, 6 queued filename, 7 dupe key, 8 hard-link path, 9 meta name.
const TEXTS: usize = 10;

/// The Rust allocation behind `NzbgetRsNzbScalars*`: the scalars first (so the
/// C pointer is this struct's address), then the strings. `None` is the C++
/// null string (only the name starts unset).
#[repr(C)]
struct NzbData {
    s: NzbScalars,
    texts: Mutex<[Option<CString>; TEXTS]>,
    coll: Mutex<Coll>,
    files: Mutex<Files>,
    // (id, kind, time, text), oldest first (see [`log_messages`])
    msgs: Mutex<std::collections::VecDeque<(u32, i32, i64, CString)>>,
    // host handles, indexed by [`Slot`]
    slots: [std::sync::atomic::AtomicUsize; SLOTS],
}

const SLOTS: usize = 5;

/// What a collection keeps in each host slot. The host owns what they
/// point to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    /// The file list.
    FileList,
    /// The post-processing job.
    Post,
    /// The direct-unpack job thread.
    UnpackJob,
    /// The live-repair job thread.
    LiveRepairJob,
    /// The message count.
    MessageCount,
}

/// The value in slot `s` of collection `p`.
pub(crate) fn slot(p: *const NzbScalars, s: Slot) -> usize {
    data(p).slot(s).load(std::sync::atomic::Ordering::SeqCst)
}

/// Stores `v` in slot `s` of collection `p`; returns the previous value.
pub(crate) fn swap_slot(p: *const NzbScalars, s: Slot, v: usize) -> usize {
    data(p).slot(s).swap(v, std::sync::atomic::Ordering::SeqCst)
}

/// A file that finished downloading and left the file list.
struct Completed {
    id: i32,
    // 0 none, 1 success, 2 partial, 3 failure
    status: i32,
    crc: u32,
    par: bool,
    filename: CString,
    origname: CString,
    hash16k: CString,
    par_set_id: CString,
}

/// Byte ranges of a completed file no server could supply, kept for stream
/// repair from a duplicate posting (see option DupeArticleFallback).
struct RepairJob {
    file_id: i32,
    filename: CString,
    decoded_size: i64,
    // encoded failed size and missed size, captured while the file was queued
    failed_size: i64,
    missed_size: i64,
    failed_articles: i32,
    par: bool,
    // (offset, size) pairs, as the host passes them
    holes: Vec<[i64; 2]>,
    steps_hash: u64,
    // the live pass already tried this job once (not persisted)
    live_attempted: bool,
}

/// Completed files and stream-repair jobs (see [`completed_files`] and [`repair_jobs`]).
#[derive(Default)]
struct Files {
    completed: Vec<Completed>,
    jobs: Vec<RepairJob>,
    // archives unpacked so far (cleared when post-processing ends)
    extracted: Vec<CString>,
}

/// A collection's small lists: parameters, script statuses, blocked donors
/// and server stats.
#[derive(Default)]
struct Coll {
    // post-processing parameters, names compared case-insensitively
    params: Vec<(CString, CString)>,
    // (script name, status)
    script_statuses: Vec<(CString, i32)>,
    // donors this collection must not borrow from again
    blocked_donors: BTreeSet<i32>,
    // (server id, success, failed): totals and current-session counts
    stats: [Vec<[i32; 3]>; 2],
}

impl Coll {
    fn param(&self, name: &[u8]) -> Option<&CStr> {
        self.params
            .iter()
            .find(|(n, _)| n.to_bytes().eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_c_str())
    }

    fn set_param(&mut self, name: &CStr, value: &CStr) {
        let pos = self
            .params
            .iter()
            .position(|(n, _)| n.to_bytes().eq_ignore_ascii_case(name.to_bytes()));
        match (value.to_bytes().is_empty(), pos) {
            (true, Some(i)) => {
                self.params.remove(i);
            }
            (false, Some(i)) => self.params[i].1 = value.to_owned(),
            (false, None) => self.params.push((name.to_owned(), value.to_owned())),
            (true, None) => {}
        }
    }
}

/// How a server stat changes the stored counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatOp {
    Set = 0,
    Add = 1,
    Subtract = 2,
}

impl StatOp {
    /// The op for its wire code (0 set, 1 add, 2 subtract).
    pub(crate) fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Set),
            1 => Some(Self::Add),
            2 => Some(Self::Subtract),
            _ => None,
        }
    }

    /// The `(success, failed)` counts after applying `delta` to `cur`.
    pub(crate) fn apply(self, cur: (i32, i32), delta: (i32, i32)) -> (i32, i32) {
        match self {
            Self::Set => delta,
            Self::Add => (cur.0.wrapping_add(delta.0), cur.1.wrapping_add(delta.1)),
            Self::Subtract => (cur.0.wrapping_sub(delta.0), cur.1.wrapping_sub(delta.1)),
        }
    }
}

/// Applies a server-stat operation to `list`.
fn stat_op(list: &mut Vec<[i32; 3]>, server_id: i32, success: i32, failed: i32, op: StatOp) {
    let i = match list.iter().position(|s| s[0] == server_id) {
        Some(i) => i,
        None => {
            list.push([server_id, 0, 0]);
            list.len() - 1
        }
    };
    let (s, f) = op.apply((list[i][1], list[i][2]), (success, failed));
    list[i][1] = s;
    list[i][2] = f;
}

fn cstr<'a>(p: *const c_char) -> &'a CStr {
    if p.is_null() {
        c""
    } else {
        unsafe { CStr::from_ptr(p) }
    }
}

fn data<'a>(p: *const NzbScalars) -> &'a NzbData {
    unsafe { &*p.cast::<NzbData>() }
}

/// The model object behind a queue handle; `None` for [`NzbRef::NONE`].
fn nzb_data<'a>(nzb: NzbRef) -> Option<&'a NzbData> {
    // A live handle always names an NZB the queue owns, as for the raw ops.
    nzb.is_some().then(|| data(nzb.addr() as *const NzbScalars))
}

impl NzbData {
    fn slot(&self, s: Slot) -> &std::sync::atomic::AtomicUsize {
        &self.slots[s as usize]
    }

    fn coll(&self) -> std::sync::MutexGuard<'_, Coll> {
        self.coll.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn files(&self) -> std::sync::MutexGuard<'_, Files> {
        self.files
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// `bytes` up to its first NUL, as a C string (names and values never hold NULs).
fn c_string(bytes: &[u8]) -> CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    CString::new(&bytes[..end]).unwrap_or_default()
}

/// The post-processing parameter `name` (compared case-insensitively).
pub(crate) fn param(nzb: NzbRef, name: &[u8]) -> Option<Vec<u8>> {
    let coll = nzb_data(nzb)?.coll();
    coll.param(name).map(|v| v.to_bytes().to_vec())
}

/// Sets `name` to `value`; an empty or missing value removes it.
pub(crate) fn set_param(nzb: NzbRef, name: &[u8], value: Option<&[u8]>) {
    if let Some(d) = nzb_data(nzb) {
        d.coll().set_param(&c_string(name), &c_string(value.unwrap_or_default()));
    }
}

/// Copies every parameter of `src` onto `dest`, overwriting same-named ones.
pub(crate) fn copy_params(dest: NzbRef, src: NzbRef) {
    let (Some(dest), Some(src)) = (nzb_data(dest), nzb_data(src)) else {
        return;
    };
    // `src` may be `dest`: copy before taking the destination's lock.
    let params = src.coll().params.clone();
    let mut coll = dest.coll();
    for (n, v) in &params {
        coll.set_param(n, v);
    }
}

/// Whether `nzb` must not borrow from donor `donor_id` again.
pub(crate) fn donor_blocked(nzb: NzbRef, donor_id: i32) -> bool {
    nzb_data(nzb).is_some_and(|d| d.coll().blocked_donors.contains(&donor_id))
}

/// Bars `nzb` from borrowing from donor `donor_id` again.
pub(crate) fn block_donor(nzb: NzbRef, donor_id: i32) {
    if let Some(d) = nzb_data(nzb) {
        d.coll().blocked_donors.insert(donor_id);
    }
}

/// The post-processing scripts that ran, as (script name, status), in run order.
pub(crate) fn script_statuses(nzb: NzbRef) -> Vec<(Vec<u8>, ScriptStatus)> {
    nzb_data(nzb).map_or_else(Vec::new, |d| {
        let coll = d.coll();
        coll.script_statuses
            .iter()
            .map(|(n, st)| (n.to_bytes().to_vec(), ScriptStatus::from(*st)))
            .collect()
    })
}

/// All parameters as (name, value), in the order they were added.
pub(crate) fn params(nzb: NzbRef) -> Vec<(Vec<u8>, Vec<u8>)> {
    nzb_data(nzb).map_or_else(Vec::new, |d| {
        let coll = d.coll();
        coll.params.iter().map(|(n, v)| (n.to_bytes().to_vec(), v.to_bytes().to_vec())).collect()
    })
}

/// One server's article counts for a download.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ServerStat {
    pub server_id: i32,
    pub success: i32,
    pub failed: i32,
}

impl From<[i32; 3]> for ServerStat {
    fn from([server_id, success, failed]: [i32; 3]) -> Self {
        Self {
            server_id,
            success,
            failed,
        }
    }
}

/// Which of a download's two server-stat lists: all-time totals, or the
/// counts since the download was last loaded or resumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatList {
    Total = 0,
    Current = 1,
}

/// A download's per-server article counts, oldest server first.
pub(crate) fn server_stats(nzb: NzbRef, which: StatList) -> Vec<ServerStat> {
    nzb_data(nzb).map_or_else(Vec::new, |d| {
        d.coll().stats[which as usize]
            .iter()
            .copied()
            .map(ServerStat::from)
            .collect()
    })
}

/// Sets the all-time counts of one server (NzbInfo::ServerStats::StatOp set).
pub(crate) fn set_server_stat(nzb: NzbRef, stat: ServerStat) {
    if let Some(d) = nzb_data(nzb) {
        let mut coll = d.coll();
        stat_op(
            &mut coll.stats[StatList::Total as usize],
            stat.server_id,
            stat.success,
            stat.failed,
            StatOp::Set,
        );
    }
}

/// Copies the all-time counts into the current-session counts.
pub(crate) fn sync_current_server_stats(nzb: NzbRef) {
    if let Some(d) = nzb_data(nzb) {
        let mut coll = d.coll();
        let totals = coll.stats[StatList::Total as usize].clone();
        for e in totals {
            stat_op(
                &mut coll.stats[StatList::Current as usize],
                e[0],
                e[1],
                e[2],
                StatOp::Set,
            );
        }
    }
}

/// Applies `op` with `stat` to a download's `which` list.
pub(crate) fn apply_server_stat(nzb: NzbRef, which: StatList, op: StatOp, stat: ServerStat) {
    if let Some(d) = nzb_data(nzb) {
        stat_op(
            &mut d.coll().stats[which as usize],
            stat.server_id,
            stat.success,
            stat.failed,
            op,
        );
    }
}

/// Adds `src`'s totals and current counts to `nzb`'s.
fn add_server_stats_from(nzb: NzbRef, src: NzbRef) {
    let (Some(d), Some(o)) = (nzb_data(nzb), nzb_data(src)) else {
        return;
    };
    // copy first: src may be nzb
    let other = o.coll().stats.clone();
    let mut c = d.coll();
    for (dst, list) in c.stats.iter_mut().zip(other) {
        for e in list {
            stat_op(dst, e[0], e[1], e[2], StatOp::Add);
        }
    }
}

/// The scripts' total status.
fn script_total_status(nzb: NzbRef) -> ScriptStatus {
    nzb_data(nzb).map_or(ScriptStatus::default(), |d| {
        let c = d.coll();
        crate::downloadinfo::script_calc_total_status(
            c.script_statuses
                .iter()
                .map(|&(_, s)| ScriptStatus::from(s)),
        )
    })
}

/// Records how a post-processing script ended.
pub(crate) fn push_script_status(nzb: NzbRef, name: &[u8], status: ScriptStatus) {
    if let Some(d) = nzb_data(nzb) {
        d.coll()
            .script_statuses
            .push((c_string(name), status.into()));
    }
}

/// The archives unpacked so far, in unpack order.
pub(crate) fn extracted_archives(nzb: NzbRef) -> Vec<Vec<u8>> {
    nzb_data(nzb).map_or_else(Vec::new, |d| {
        d.files()
            .extracted
            .iter()
            .map(|a| a.to_bytes().to_vec())
            .collect()
    })
}

/// Records an unpacked archive.
pub(crate) fn push_extracted_archive(nzb: NzbRef, name: &[u8]) {
    if let Some(d) = nzb_data(nzb) {
        d.files().extracted.push(c_string(name));
    }
}

/// Renames the completed file `old` to `new`, keeping the first name as the
/// original one; false when no completed file is called `old`.
pub(crate) fn rename_completed_file(nzb: NzbRef, old: &[u8], new: &[u8]) -> bool {
    let Some(d) = nzb_data(nzb) else { return false };
    if old.is_empty() || new.is_empty() {
        return false;
    }
    let mut files = d.files();
    let Some(c) = files
        .completed
        .iter_mut()
        .find(|c| crate::downloadinfo::completed_same_filename(c.filename.to_bytes(), old))
    else {
        return false;
    };
    if c.origname.to_bytes().is_empty() {
        c.origname = c.filename.clone();
    }
    c.filename = c_string(new);
    true
}

/// The directory the download moves to when done (NzbInfo::BuildFinalDirName).
pub(crate) fn build_final_dir_name(nzb: NzbRef) -> Vec<u8> {
    match nzb_data(nzb) {
        Some(_) => unsafe { final_dir_name(nzb.addr() as *const NzbScalars) },
        None => Vec::new(),
    }
}

/// The path of the hard link to the queued NZB file, empty when none.
pub(crate) fn hard_link_path(nzb: NzbRef) -> Vec<u8> {
    nzb_data(nzb).map_or_else(Vec::new, |_| text_bytes(nzb.addr() as *const NzbScalars, 8))
}

/// The name the download's NZB file carries in its meta data, empty when none.
pub(crate) fn meta_name(nzb: NzbRef) -> Vec<u8> {
    nzb_data(nzb).map_or_else(Vec::new, |_| text_bytes(nzb.addr() as *const NzbScalars, 9))
}

pub(crate) fn set_meta_name(nzb: NzbRef, name: &[u8]) {
    if nzb_data(nzb).is_some() {
        set_text_bytes(nzb.addr() as *const NzbScalars, 9, name);
    }
}

/// The direct-unpack job handle kept with the download, 0 for none.
pub(crate) fn unpack_thread(nzb: NzbRef) -> usize {
    nzb_data(nzb).map_or(0, |d| {
        d.slot(Slot::UnpackJob)
            .load(std::sync::atomic::Ordering::SeqCst)
    })
}

/// Whether a file still downloading has its output file open.
pub(crate) fn has_busy_output_file(nzb: NzbRef) -> bool {
    nzb_data(nzb).is_some() && unsafe { has_active_output(nzb.addr() as *const NzbScalars) }
}

/// A logged message kept with a download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CachedMessage {
    pub id: u32,
    pub kind: i32,
    pub time: i64,
    pub text: CString,
}

/// Which cached messages to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageRange {
    /// From the message with this id on (all when it is older than every kept one).
    FromId(u32),
    /// The newest this many.
    Last(usize),
}

/// The download's cached messages in `range`, oldest first.
pub(crate) fn cached_messages(nzb: NzbRef, range: MessageRange) -> Vec<CachedMessage> {
    let Some(d) = nzb_data(nzb) else {
        return Vec::new();
    };
    let m = d
        .msgs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let start = match range {
        MessageRange::FromId(id) => m
            .front()
            .map_or(0, |first| id.saturating_sub(first.0) as usize),
        MessageRange::Last(n) => m.len() - n.min(m.len()),
    };
    m.iter()
        .skip(start)
        .map(|(id, kind, time, text)| CachedMessage {
            id: *id,
            kind: *kind,
            time: *time,
            text: text.clone(),
        })
        .collect()
}

/// Appends `src`'s cached messages to `dest` with fresh ids and the current
/// time, keeping at most the configured log buffer.
pub(crate) fn copy_cached_messages(dest: NzbRef, src: NzbRef) {
    let (Some(_), Some(_)) = (nzb_data(dest), nzb_data(src)) else {
        return;
    };
    let log_buffer = crate::options::active_options().map_or(0, |o| o.scalars().log_buffer);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as c_int);
    if dest == src {
        return;
    }
    let copied: Vec<(i32, CString)> = {
        let o = data(scalars(src))
            .msgs
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        o.iter().map(|m| (m.1, m.3.clone())).collect()
    };
    log_messages(scalars(dest), i64::from(now), log_buffer, copied);
}

/// An integer or flag field of a download (flags take 0 or 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NzbInt {
    Id = 0,
    DeleteStatus = 1,
    DeletePaused = 2,
    MarkStatus = 3,
    DupeHint = 4,
    DupeScore = 5,
    Priority = 6,
    FullHash = 7,
    FilteredHash = 8,
    Changed = 9,
    AllFirst = 10,
    HealthPaused = 11,
    DeadPickProbed = 12,
    DupeParDeferState = 13,
    DupeNoParNoted = 14,
    DirectRenameStatus = 15,
    ActiveDownloads = 16,
    CurSuccessArticles = 17,
    CurFailedArticles = 18,
    DupeRecoveredArticles = 19,
    DupeAttemptedArticles = 20,
    DupeUnsourcedArticles = 21,
    DupeFailedRun = 22,
    DupeFailoverChecked = 23,
    RemainingParCount = 24,
    WaitingPar = 25,
    LoadingPar = 26,
    Deleting = 27,
    Parking = 28,
    AvoidHistory = 29,
    CleanupDisk = 60,
    DupeMode = 61,
    ParStatus = 62,
    UnpackStatus = 63,
    DirectUnpackStatus = 64,
    PostTotalSec = 65,
    UrlStatus = 66,
    ParSec = 67,
    ParFull = 68,
    RepairSec = 69,
    ExtraParBlocks = 70,
    DupeRecoveredHoles = 71,
    ParRenameStatus = 72,
    UnpackCleanedUpDisk = 73,
    UnpackSec = 74,
    MoveStatus = 75,
    CleanupStatus = 76,
    RarRenameStatus = 77,
    PostDownloadRenamingStatus = 78,
    PostUnpackRenamingStatus = 79,
    Kind = 81,
    FeedId = 82,
    AddUrlPaused = 83,
    AutoCategory = 84,
    DesiredServerId = 85,
    SkipScriptProcessing = 86,
    SkipDiskWrite = 87,
    ExtraPriority = 88,
    ManyDupeFiles = 89,
    ParkedFileCount = 90,
    Reprocess = 91,
    ParRepaired = 92,
    UnpackTried = 93,
    LastUnpackStatus = 94,
    DownloadSec = 99,
    FailedArticles = 100,
    SuccessArticles = 101,
    FileCount = 106,
    MessageCount = 107,
    TotalArticles = 108,
    PostStage = 109,
    PostForceFlags = 110,
    LiveUnpackSec = 114,
    LiveParSec = 115,
}

/// A 64-bit size or time field of a download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NzbI64 {
    RemainingSize = 30,
    PausedSize = 31,
    CurSuccessSize = 32,
    CurFailedSize = 33,
    ParCurSuccessSize = 34,
    ParCurFailedSize = 35,
    DownloadedSize = 36,
    ParSize = 37,
    ParFailedSize = 38,
    DupeRecoveredBytes = 39,
    FailedSize = 47,
    SuccessSize = 48,
    ParSuccessSize = 49,
    Size = 102,
    MinTime = 103,
    MaxTime = 104,
}

/// A text field of a download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NzbStr {
    Name = 40,
    Filename = 41,
    Category = 42,
    DestDir = 43,
    DupeKey = 44,
    QueuedFilename = 45,
    HardLinkPath = 46,
    FinalDir = 59,
    Url = 96,
}

/// An update a download makes to itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NzbAction {
    BuildDestDir = 50,
    UpdateMinMaxTime = 51,
    EnterPostProcess = 54,
    LeavePostProcess = 55,
    ClearScriptStatuses = 56,
    DisconnectThreads = 57,
    ClearLiveRepairThread = 58,
    ClearUnpackThread = 80,
    ClearExtractedArchives = 95,
    ClearFileList = 97,
    UpdateCurrentStats = 98,
    ClearParams = 111,
    ClearRepairJobs = 112,
    ClearCompletedFiles = 113,
}

/// The download behind `nzb` as its scalars and model data; `None` for
/// [`NzbRef::NONE`].
///
/// The scalars are plain memory changed only under the queue lock, which
/// every caller of the field setters holds.
fn scalars_mut<'a>(nzb: NzbRef) -> Option<(*mut NzbScalars, &'a mut NzbScalars, &'a NzbData)> {
    let d = nzb_data(nzb)?;
    let p = nzb.addr() as *mut NzbScalars;
    Some((p, unsafe { &mut *p }, d))
}

/// Sets an integer or flag field (NzbInfo's int setters).
pub(crate) fn set_int(nzb: NzbRef, field: NzbInt, v: i32) {
    use std::sync::atomic::Ordering::SeqCst;
    let Some((_, s, d)) = scalars_mut(nzb) else {
        return;
    };
    match field {
        NzbInt::Id => set_id(s, v),
        NzbInt::DeleteStatus => s.deleteStatus = v,
        NzbInt::DeletePaused => s.deletePaused = v != 0,
        NzbInt::MarkStatus => s.markStatus = v,
        NzbInt::DupeHint => s.dupeHint = v,
        NzbInt::DupeScore => s.dupeScore = v,
        NzbInt::Priority => s.priority = v,
        NzbInt::FullHash => s.fullContentHash = v as u32,
        NzbInt::FilteredHash => s.filteredContentHash = v as u32,
        NzbInt::Changed => s.changed = v != 0,
        NzbInt::AllFirst => s.allFirst = v != 0,
        NzbInt::HealthPaused => s.healthPaused = v != 0,
        NzbInt::DeadPickProbed => s.deadPickProbed = v != 0,
        NzbInt::DupeParDeferState => s.dupeParDeferState = v,
        NzbInt::DupeNoParNoted => s.dupeNoParNoted = v != 0,
        NzbInt::DirectRenameStatus => s.directRenameStatus = v,
        NzbInt::ActiveDownloads => set_active_downloads(s, v, unix_now()),
        NzbInt::CurSuccessArticles => s.currentSuccessArticles = v,
        NzbInt::CurFailedArticles => s.currentFailedArticles = v,
        NzbInt::DupeRecoveredArticles => s.dupeRecoveredArticles = v,
        NzbInt::DupeAttemptedArticles => s.dupeAttemptedArticles = v,
        NzbInt::DupeUnsourcedArticles => s.dupeUnsourcedArticles = v,
        NzbInt::DupeFailedRun => s.dupeFailedRun = v,
        NzbInt::DupeFailoverChecked => s.dupeFailoverChecked = v,
        NzbInt::RemainingParCount => s.remainingParCount = v,
        NzbInt::WaitingPar => s.waitingPar = v != 0,
        NzbInt::LoadingPar => s.loadingPar = v != 0,
        NzbInt::Deleting => s.deleting = v != 0,
        NzbInt::Parking => s.parking = v != 0,
        NzbInt::AvoidHistory => s.avoidHistory = v != 0,
        NzbInt::CleanupDisk => s.cleanupDisk = v != 0,
        NzbInt::DupeMode => s.dupeMode = v,
        NzbInt::ParStatus => s.parStatus = v,
        NzbInt::UnpackStatus => s.unpackStatus = v,
        NzbInt::DirectUnpackStatus => s.directUnpackStatus = v,
        NzbInt::PostTotalSec => s.postTotalSec = v,
        NzbInt::UrlStatus => s.urlStatus = v,
        NzbInt::ParSec => s.parSec = v,
        NzbInt::ParFull => s.parFull = v != 0,
        NzbInt::RepairSec => s.repairSec = v,
        NzbInt::ExtraParBlocks => s.extraParBlocks = v,
        NzbInt::DupeRecoveredHoles => s.dupeRecoveredHoles = v,
        NzbInt::ParRenameStatus => s.parRenameStatus = v,
        NzbInt::UnpackCleanedUpDisk => s.unpackCleanedUpDisk = v != 0,
        NzbInt::UnpackSec => s.unpackSec = v,
        NzbInt::MoveStatus => s.moveStatus = v,
        NzbInt::CleanupStatus => s.cleanupStatus = v,
        NzbInt::RarRenameStatus => s.rarRenameStatus = v,
        NzbInt::PostDownloadRenamingStatus => s.postDownloadRenamingStatus = v,
        NzbInt::PostUnpackRenamingStatus => s.postUnpackRenamingStatus = v,
        NzbInt::Kind => s.kind = v,
        NzbInt::FeedId => s.feedId = v,
        NzbInt::AddUrlPaused => s.addUrlPaused = v != 0,
        NzbInt::AutoCategory => s.autoCategory = v != 0,
        NzbInt::DesiredServerId => s.desiredServerId = v,
        NzbInt::SkipScriptProcessing => s.skipScriptProcessing = v != 0,
        NzbInt::SkipDiskWrite => s.skipDiskWrite = v != 0,
        NzbInt::ExtraPriority => s.extraPriority = v,
        NzbInt::ManyDupeFiles => s.manyDupeFiles = v != 0,
        NzbInt::ParkedFileCount => s.parkedFileCount = v,
        NzbInt::Reprocess => s.reprocess = v != 0,
        NzbInt::ParRepaired => s.parRepaired = v != 0,
        NzbInt::UnpackTried => s.unpackTried = v != 0,
        NzbInt::LastUnpackStatus => s.lastUnpackStatus = v,
        NzbInt::DownloadSec => s.downloadSec = v,
        NzbInt::FailedArticles => s.failedArticles = v,
        NzbInt::SuccessArticles => s.successArticles = v,
        NzbInt::FileCount => s.fileCount = v,
        NzbInt::MessageCount => {
            d.slot(Slot::MessageCount)
                .store(v as isize as usize, SeqCst);
        }
        NzbInt::TotalArticles => s.totalArticles = v,
        NzbInt::LiveUnpackSec => s.liveUnpackSec = v,
        NzbInt::LiveParSec => s.liveParSec = v,
        NzbInt::PostStage | NzbInt::PostForceFlags => {
            let post = d.slot(Slot::Post).load(SeqCst) as *const Post;
            if !post.is_null() {
                if field == NzbInt::PostStage {
                    post_set(post, PostField::Int(PostInt::Stage), i64::from(v));
                } else {
                    post_set(
                        post,
                        PostField::Int(PostInt::ForceParFull),
                        i64::from(v & 1 != 0),
                    );
                    post_set(
                        post,
                        PostField::Int(PostInt::ForceRepair),
                        i64::from(v & 2 != 0),
                    );
                }
            }
        }
    }
}

/// Sets a 64-bit size or time field.
pub(crate) fn set_i64(nzb: NzbRef, field: NzbI64, v: i64) {
    let Some((_, s, _)) = scalars_mut(nzb) else {
        return;
    };
    match field {
        NzbI64::RemainingSize => s.remainingSize = v,
        NzbI64::PausedSize => s.pausedSize = v,
        NzbI64::CurSuccessSize => s.currentSuccessSize = v,
        NzbI64::CurFailedSize => s.currentFailedSize = v,
        NzbI64::ParCurSuccessSize => s.parCurrentSuccessSize = v,
        NzbI64::ParCurFailedSize => s.parCurrentFailedSize = v,
        NzbI64::DownloadedSize => s.downloadedSize = v,
        NzbI64::ParSize => s.parSize = v,
        NzbI64::ParFailedSize => s.parFailedSize = v,
        NzbI64::DupeRecoveredBytes => s.dupeRecoveredBytes = v,
        NzbI64::FailedSize => s.failedSize = v,
        NzbI64::SuccessSize => s.successSize = v,
        NzbI64::ParSuccessSize => s.parSuccessSize = v,
        NzbI64::Size => s.size = v,
        NzbI64::MinTime => s.minTime = v,
        NzbI64::MaxTime => s.maxTime = v,
    }
}

/// Sets a text field; `None` unsets it, which reads as empty (the dupe key,
/// hard-link path and final directory store an empty text instead).
pub(crate) fn set_str(nzb: NzbRef, field: NzbStr, value: Option<&[u8]>) {
    let Some((p, _, _)) = scalars_mut(nzb) else {
        return;
    };
    let value = value.map(c_string);
    let s1 = value.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
    let or_empty = |s: *const c_char| if s.is_null() { c"".as_ptr() } else { s };
    match field {
        NzbStr::Name => unsafe { nzbget_rs_nzb_set_text(p, 2, s1, -1) },
        NzbStr::Filename => unsafe { set_filename(p, s1) },
        NzbStr::Category => unsafe { nzbget_rs_nzb_set_text(p, 5, s1, -1) },
        NzbStr::DestDir => unsafe { nzbget_rs_nzb_set_text(p, 3, s1, -1) },
        NzbStr::DupeKey => unsafe { nzbget_rs_nzb_set_text(p, 7, or_empty(s1), -1) },
        NzbStr::QueuedFilename => unsafe { nzbget_rs_nzb_set_text(p, 6, s1, -1) },
        NzbStr::HardLinkPath => unsafe { nzbget_rs_nzb_set_text(p, 8, or_empty(s1), -1) },
        NzbStr::FinalDir => unsafe { nzbget_rs_nzb_set_text(p, 4, or_empty(s1), -1) },
        NzbStr::Url => unsafe { set_url(p, s1) },
    }
}

/// Which of a download's totals a finished file is added to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileTotals {
    Completed,
    Deleted,
}

/// Adds `file`'s sizes and article counts to the download's completed or
/// deleted totals (NzbInfo::UpdateCompletedStats / UpdateDeletedStats).
pub(crate) fn add_file_totals(
    nzb: NzbRef,
    file: crate::queuesubsys_host::FileRef,
    totals: FileTotals,
) {
    let Some((p, _, _)) = scalars_mut(nzb) else {
        return;
    };
    let f = file.addr() as *const crate::qfile::File;
    match totals {
        FileTotals::Completed => unsafe { update_completed_stats(p, f) },
        FileTotals::Deleted => unsafe { update_deleted_stats(p, f) },
    }
}

/// Runs one of the download's self-updates.
pub(crate) fn act(nzb: NzbRef, action: NzbAction) {
    use std::sync::atomic::Ordering::SeqCst;
    let Some((p, _, d)) = scalars_mut(nzb) else {
        return;
    };
    match action {
        NzbAction::BuildDestDir => unsafe { build_dest_dir_name(p) },
        NzbAction::UpdateMinMaxTime => unsafe { update_min_max_time(p) },
        NzbAction::EnterPostProcess => unsafe { enter_post_process(p) },
        NzbAction::LeavePostProcess => unsafe { leave_post_process(p) },
        NzbAction::ClearScriptStatuses => d.coll().script_statuses.clear(),
        NzbAction::DisconnectThreads => {
            d.slot(Slot::UnpackJob).store(0, SeqCst);
            d.slot(Slot::LiveRepairJob).store(0, SeqCst);
        }
        NzbAction::ClearLiveRepairThread => d.slot(Slot::LiveRepairJob).store(0, SeqCst),
        NzbAction::ClearUnpackThread => d.slot(Slot::UnpackJob).store(0, SeqCst),
        NzbAction::ClearExtractedArchives => d.files().extracted.clear(),
        NzbAction::ClearFileList => {
            if file_list(p) != 0 {
                ptr_list_op(file_list(p), 9, 0, 0, std::ptr::null(), 0);
            }
        }
        NzbAction::UpdateCurrentStats => unsafe { update_current_stats(p) },
        NzbAction::ClearParams => d.coll().params.clear(),
        NzbAction::ClearRepairJobs => d.files().jobs.clear(),
        NzbAction::ClearCompletedFiles => d.files().completed.clear(),
    }
}

/// An integer or flag field of a post-processing job (flags take 0 or 1).
/// ParRepaired, UnpackTried and LastUnpackStatus also live on the download,
/// and setting them writes both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PostInt {
    Stage = 0,
    Working = 1,
    Deleted = 2,
    RequestParCheck = 3,
    ForceParFull = 4,
    ParCannotCover = 5,
    NeedParCheck = 6,
    NeedUnpack = 7,
    ForceRepair = 8,
    ParRepaired = 9,
    UnpackTried = 10,
    PassListTried = 11,
    LastUnpackStatus = 12,
    FileProgress = 13,
    /// Kept in 0..=1000.
    StageProgress = 14,
    StreamRepairDone = 15,
    StreamRepairRecovered = 16,
}

impl PostInt {
    /// Whether the field is a flag (stored as 0 or 1).
    const fn is_flag(self) -> bool {
        !matches!(
            self,
            Self::Stage | Self::LastUnpackStatus | Self::FileProgress | Self::StageProgress
        )
    }
}

/// The live post job behind a handle; `None` for [`PostRef::NONE`].
fn post_of(post: crate::queuesubsys_host::PostRef) -> Option<*const Post> {
    post.is_some().then(|| post.addr() as *const Post)
}

/// Sets an integer or flag field of a post job.
pub(crate) fn post_set_int(post: crate::queuesubsys_host::PostRef, field: PostInt, v: i32) {
    if let Some(p) = post_of(post) {
        let v = if field.is_flag() {
            i64::from(v != 0)
        } else {
            i64::from(v)
        };
        post_set(p, PostField::Int(field), v);
    }
}

/// Sets when post-processing started.
pub(crate) fn post_set_start_time(post: crate::queuesubsys_host::PostRef, time: i64) {
    if let Some(p) = post_of(post) {
        post_set(p, PostField::StartTime, time);
    }
}

/// Starts a new stage at `time`: progress and label are cleared.
pub(crate) fn post_begin_stage(post: crate::queuesubsys_host::PostRef, time: i64) {
    if let Some(p) = post_of(post) {
        post_set(p, PostField::StageTime, time);
        post_set(p, PostField::Int(PostInt::StageProgress), 0);
        post_set(p, PostField::Int(PostInt::FileProgress), 0);
        post_set_label(p, c"");
    }
}

/// Sets when the current stage started.
pub(crate) fn post_set_stage_time(post: crate::queuesubsys_host::PostRef, time: i64) {
    if let Some(p) = post_of(post) {
        post_set(p, PostField::StageTime, time);
    }
}

/// Sets the progress label.
pub(crate) fn post_set_progress_label(post: crate::queuesubsys_host::PostRef, label: &[u8]) {
    if let Some(p) = post_of(post) {
        post_set_label(p, &c_string(label));
    }
}

fn post_set_label(p: *const Post, label: &CStr) {
    unsafe { &*p }
        .lists
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .label = label.to_owned();
}

/// Records a file the par check repaired or renamed.
pub(crate) fn post_push_parred_file(post: crate::queuesubsys_host::PostRef, name: &[u8]) {
    if let Some(p) = post_of(post) {
        unsafe { &*p }
            .lists
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .parred
            .push(c_string(name));
    }
}

/// The files the par check repaired or renamed, in order.
pub(crate) fn post_parred_files(post: crate::queuesubsys_host::PostRef) -> Vec<Vec<u8>> {
    post_of(post).map_or_else(Vec::new, |p| {
        let l = unsafe { &*p }
            .lists
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        l.parred.iter().map(|f| f.to_bytes().to_vec()).collect()
    })
}

/// The archives unpacked so far: the download's list when the job has a
/// download, else the job's own.
pub(crate) fn post_extracted_archives(post: crate::queuesubsys_host::PostRef) -> Vec<Vec<u8>> {
    use std::sync::atomic::Ordering::SeqCst;
    let Some(p) = post_of(post) else {
        return Vec::new();
    };
    let post = unsafe { &*p };
    let nzb = post.nzb.load(SeqCst);
    if nzb != 0 {
        return extracted_archives(NzbRef::from_addr(nzb));
    }
    let l = post
        .lists
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    l.extracted.iter().map(|a| a.to_bytes().to_vec()).collect()
}

/// Forgets the archives unpacked so far (the download's list when the job
/// has a download, else the job's own).
pub(crate) fn post_clear_extracted_archives(post: crate::queuesubsys_host::PostRef) {
    use std::sync::atomic::Ordering::SeqCst;
    let Some(p) = post_of(post) else { return };
    let post = unsafe { &*p };
    match post.nzb.load(SeqCst) {
        0 => post
            .lists
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extracted
            .clear(),
        nzb => {
            if let Some(d) = nzb_data(NzbRef::from_addr(nzb)) {
                d.files().extracted.clear();
            }
        }
    }
}

pub extern "C" fn nzbget_rs_nzb_scalars_new() -> *mut NzbScalars {
    let mut texts: [Option<CString>; TEXTS] = Default::default();
    for (i, t) in texts.iter_mut().enumerate() {
        if i != 2 {
            *t = Some(CString::default());
        }
    }
    let data = Box::new(NzbData {
        s: NzbScalars::default(),
        texts: Mutex::new(texts),
        coll: Mutex::new(Coll::default()),
        files: Mutex::new(Files::default()),
        msgs: Mutex::new(std::collections::VecDeque::new()),
        slots: std::array::from_fn(|_| std::sync::atomic::AtomicUsize::new(0)),
    });
    Box::into_raw(data).cast()
}

unsafe extern "C" fn drop_file(p: usize) {
    unsafe { crate::qfile::nzbget_rs_file_free(p as *mut crate::qfile::File) };
}

/// The `NzbInfo` constructor: the next id and an empty file list.
///
/// # Safety
/// `p` is from `nzbget_rs_nzb_scalars_new` and not initialised yet.
pub unsafe extern "C" fn nzbget_rs_nzb_init(p: *mut NzbScalars) {
    unsafe { (*p).id = crate::idgen::NZB_IDS.next() };
    let files = crate::qlist::nzbget_rs_ptrlist_new(drop_file);
    swap_slot(p, Slot::FileList, files as usize);
}

/// # Safety
/// `p` is null or from `nzbget_rs_nzb_scalars_new`, freed once.
pub unsafe extern "C" fn nzbget_rs_nzb_scalars_free(p: *mut NzbScalars) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(p.cast::<NzbData>()) });
    }
}

/// Text field `id` (null when unset); valid until that field changes.
///
/// # Safety
/// `p` is from `nzbget_rs_nzb_scalars_new`.
pub unsafe extern "C" fn nzbget_rs_nzb_text(p: *const NzbScalars, id: c_int) -> *const c_char {
    let data = unsafe { &*p.cast::<NzbData>() };
    let texts = data.texts.lock().unwrap_or_else(|e| e.into_inner());
    texts
        .get(id as usize)
        .and_then(|t| t.as_ref())
        .map_or(std::ptr::null(), |t| t.as_ptr())
}

/// Sets text field `id` to the first `len` bytes of `s` (`len` < 0: up to its
/// NUL); a null `s` makes it unset.
///
/// # Safety
/// `p` is from `nzbget_rs_nzb_scalars_new`; `s` is null or readable as stated.
pub unsafe extern "C" fn nzbget_rs_nzb_set_text(
    p: *const NzbScalars,
    id: c_int,
    s: *const c_char,
    len: isize,
) {
    let value = if s.is_null() {
        None
    } else if len < 0 {
        Some(unsafe { CStr::from_ptr(s) }.to_owned())
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(s.cast::<u8>(), len as usize) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        Some(CString::new(&bytes[..end]).unwrap_or_default())
    };
    let data = unsafe { &*p.cast::<NzbData>() };
    let mut texts = data.texts.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = texts.get_mut(id as usize) {
        *slot = value;
    }
}

/// Post-processing state of a collection. The C++ PostInfo is this
/// allocation (a PostInfo* is a Post*): it has no members of its own.
pub struct Post {
    // stored values, indexed by [`PostField::index`]
    vals: [std::sync::atomic::AtomicI64; POST_VALS],
    // the collection being post-processed (an NzbScalars*), 0 for none
    nzb: std::sync::atomic::AtomicUsize,
    lists: Mutex<PostLists>,
}

const POST_VALS: usize = 20;

#[derive(Default)]
struct PostLists {
    label: CString,
    parred: Vec<CString>,
    // used only when the post job has no collection
    extracted: Vec<CString>,
}

pub extern "C" fn nzbget_rs_post_new() -> *mut Post {
    Box::into_raw(Box::new(Post {
        vals: std::array::from_fn(|_| std::sync::atomic::AtomicI64::new(0)),
        nzb: std::sync::atomic::AtomicUsize::new(0),
        lists: Mutex::new(PostLists::default()),
    }))
}

pub unsafe extern "C" fn nzbget_rs_post_free(p: *mut Post) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(p) });
    }
}

/// A copy of the progress label: it may change on another thread.
///
/// # Safety
/// `p` points to a live post job.
pub unsafe fn nzbget_rs_post_label(p: *const Post) -> Vec<u8> {
    let post = unsafe { &*p };
    let l = post.lists.lock().unwrap_or_else(|e| e.into_inner());
    l.label.as_bytes().to_vec()
}

/// A post job's post-processing thread ([`PostField::Thread`]), if any.
fn post_thread(p: *const Post) -> Option<JobHandle> {
    let addr = post_get(p, PostField::Thread) as usize;
    // SAFETY: PostField::Thread holds 0 or the post job's live thread.
    unsafe { JobHandle::from_addr(addr) }
}

/// The job thread in a download's slot `id`, if any.
fn slot_job(nzb: usize, which: Slot) -> Option<JobHandle> {
    if nzb == 0 {
        return None;
    }
    let addr = slot(nzb as *const NzbScalars, which);
    // SAFETY: the job slots hold 0 or a live job thread.
    unsafe { JobHandle::from_addr(addr) }
}

/// A value a post job stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PostField {
    Int(PostInt),
    StartTime,
    StageTime,
    /// The post-processing thread's address, 0 for none.
    Thread,
}

impl PostField {
    const fn index(self) -> usize {
        match self {
            Self::Int(f) => f as usize,
            Self::StartTime => 17,
            Self::StageTime => 18,
            Self::Thread => 19,
        }
    }
}

/// Sets a post job's value. Stage progress is kept in 0..=1000. Par
/// repaired, unpack tried and last unpack status are also written to the
/// job's collection, if it has one.
fn post_set(p: *const Post, field: PostField, v: i64) {
    use std::sync::atomic::Ordering::SeqCst;
    let post = unsafe { &*p };
    let v = if field == PostField::Int(PostInt::StageProgress) {
        v.clamp(0, 1000)
    } else {
        v
    };
    post.vals[field.index()].store(v, SeqCst);
    let nzb = post_nzb(p);
    if let (PostField::Int(f), false) = (field, nzb.is_null()) {
        let s = unsafe { &mut *nzb };
        match f {
            PostInt::ParRepaired => s.parRepaired = v != 0,
            PostInt::UnpackTried => s.unpackTried = v != 0,
            PostInt::LastUnpackStatus => s.lastUnpackStatus = v as i32,
            _ => {}
        }
    }
}

/// A post job's value. Par repaired, unpack tried and last unpack status
/// come from the job's collection when it has one.
fn post_get(p: *const Post, field: PostField) -> i64 {
    use std::sync::atomic::Ordering::SeqCst;
    let nzb = post_nzb(p);
    if let (PostField::Int(f), false) = (field, nzb.is_null()) {
        let s = unsafe { &*nzb };
        match f {
            PostInt::ParRepaired => return i64::from(s.parRepaired),
            PostInt::UnpackTried => return i64::from(s.unpackTried),
            PostInt::LastUnpackStatus => return i64::from(s.lastUnpackStatus),
            _ => {}
        }
    }
    unsafe { &*p }.vals[field.index()].load(SeqCst)
}

/// The collection a post job processes, null for none.
fn post_nzb(p: *const Post) -> *mut NzbScalars {
    unsafe { &*p }.nzb.load(std::sync::atomic::Ordering::SeqCst) as *mut NzbScalars
}

fn set_post_nzb(p: *const Post, nzb: *mut NzbScalars) {
    unsafe { &*p }
        .nzb
        .store(nzb as usize, std::sync::atomic::Ordering::SeqCst);
}

/// Starts post-processing for a collection (host op 32 sub-op 54): a fresh
/// post job replaces any earlier one.
///
/// # Safety
/// `p` is a live collection; the caller holds the queue lock.
pub(crate) unsafe fn enter_post_process(p: *mut NzbScalars) {
    let post = nzbget_rs_post_new();
    set_post_nzb(post, p);
    let old = swap_slot(p, Slot::Post, post as usize);
    unsafe { nzbget_rs_post_free(old as *mut Post) };
}

/// Ends post-processing for a collection (host op 32 sub-op 55): frees the
/// post job and resets the per-run unpack state. Messages stay.
///
/// # Safety
/// `p` is a live collection; the caller holds the queue lock.
pub(crate) unsafe fn leave_post_process(p: *mut NzbScalars) {
    let old = swap_slot(p, Slot::Post, 0);
    unsafe { nzbget_rs_post_free(old as *mut Post) };
    let s = unsafe { &mut *p };
    s.unpackTried = false;
    s.lastUnpackStatus = 0;
    data(p).files().extracted.clear();
}

/// A copy of a post job's state, or the default for a missing handle.
pub(crate) fn post_snapshot(
    post: crate::queuesubsys_host::PostRef,
) -> crate::queuesubsys_host::PostSnap {
    use crate::status::{PostStage, UnpackStatus};
    let Some(p) = post_of(post) else {
        return crate::queuesubsys_host::PostSnap::default();
    };
    let flag = |f| post_get(p, PostField::Int(f)) != 0;
    let int = |f| post_get(p, PostField::Int(f)) as i32;
    let pt = post_thread(p);
    let nzb = post_nzb(p);
    let progress_label = unsafe { &*p }
        .lists
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .label
        .to_bytes()
        .to_vec();
    crate::queuesubsys_host::PostSnap {
        ptr: post,
        nzb_ptr: NzbRef::from_addr(nzb as usize),
        stage: PostStage::from(int(PostInt::Stage)),
        working: flag(PostInt::Working),
        deleted: flag(PostInt::Deleted),
        request_par_check: flag(PostInt::RequestParCheck),
        force_par_full: flag(PostInt::ForceParFull),
        force_repair: flag(PostInt::ForceRepair),
        stream_repair_done: flag(PostInt::StreamRepairDone),
        stream_repair_recovered: flag(PostInt::StreamRepairRecovered),
        par_cannot_cover: flag(PostInt::ParCannotCover),
        need_par_check: flag(PostInt::NeedParCheck),
        need_unpack: flag(PostInt::NeedUnpack),
        has_post_thread: pt.is_some(),
        post_thread_running: pt.is_some_and(JobHandle::is_running),
        unpack_tried: flag(PostInt::UnpackTried),
        par_repaired: flag(PostInt::ParRepaired),
        pass_list_tried: flag(PostInt::PassListTried),
        last_unpack_status: UnpackStatus::from(int(PostInt::LastUnpackStatus)),
        stage_progress: int(PostInt::StageProgress),
        file_progress: int(PostInt::FileProgress),
        start_time: post_get(p, PostField::StartTime),
        stage_time: post_get(p, PostField::StageTime),
        progress_label,
    }
}

/// Starts a direct-unpack job for collection `p` (host op 62 sub-op 8): the
/// job goes in slot 2 and the direct-unpack status becomes running (back to
/// none, with the slot cleared, when the thread cannot start).
///
/// # Safety
/// `p` is a live collection; the caller holds the queue lock.
pub unsafe extern "C" fn nzbget_rs_nzb_start_direct_unpack(p: *mut NzbScalars) {
    if p.is_null() {
        return;
    }
    let job = JobHandle::new_unpack(unsafe { (*p).id });
    swap_slot(p, Slot::UnpackJob, job.addr());
    // NzbInfo::nsRunning
    unsafe { (*p).directUnpackStatus = 1 };
    if !job.start() {
        unsafe {
            swap_slot(p, Slot::UnpackJob, 0);
            (*p).directUnpackStatus = 0;
            job.free(false);
        }
    }
}

/// Host op 62: post-processing and direct-unpack job handles (sub-op `a1`):
///  0 stop the post job p1's thread; 1 detach and free it; 2 kill and free it;
///  3 is collection p1's direct unpack active (a2 != 0: any running one);
///  4-7 direct-unpack ops on collection p2's job (4 stop, 5 collection
///  downloaded, 6 deleted, 7 file i64s[0] downloaded; p1 the coordinator);
///  8 start a direct unpack for collection p1;
///  9 stop collection p1's live repair; 10 detach and stop it;
///  11 add par file p2 (a2 deleted) to the post job p1's repair;
///  12 start post job p1's controller of kind a2.
///
/// # Safety
/// `p1`/`p2` are null or live objects of the kind the sub-op takes.
pub(crate) unsafe fn host_post_job_op(
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    i64s: *const i64,
) -> usize {
    let post = p1 as *const Post;
    match a1 {
        0 if !post.is_null() => {
            if let Some(t) = post_thread(post) {
                t.stop();
            }
        }
        1 | 2 if !post.is_null() => {
            if let Some(t) = post_thread(post) {
                post_set(post, PostField::Thread, 0);
                unsafe { t.free(a1 == 2) };
            }
        }
        3 => {
            let Some(du) = slot_job(p1, Slot::UnpackJob) else {
                return 0;
            };
            let active = a2 != 0 || (du.unpack_is_unpacking() && !du.unpack_is_waiting_volume());
            return usize::from(active);
        }
        4..=7 => {
            if let Some(du) = slot_job(p2, Slot::UnpackJob) {
                match a1 {
                    4 => du.unpack_stop(NzbRef::from_addr(p2)),
                    5 => du.unpack_nzb_downloaded(NzbRef::from_addr(p2)),
                    6 => du.unpack_nzb_deleted(NzbRef::from_addr(p2)),
                    _ => du.unpack_file_downloaded(FileRef::from_addr((unsafe { *i64s }) as usize)),
                }
            }
        }
        8 if p1 != 0 => unsafe { nzbget_rs_nzb_start_direct_unpack(p1 as *mut NzbScalars) },
        9 => {
            if let Some(t) = slot_job(p1, Slot::LiveRepairJob) {
                t.stop();
            }
        }
        10 if p1 != 0 => {
            let addr = swap_slot(p1 as *const NzbScalars, Slot::LiveRepairJob, 0);
            // SAFETY: the slot held 0 or the live repair's thread.
            if let Some(t) = unsafe { JobHandle::from_addr(addr) } {
                t.stop();
            }
        }
        11 if !post.is_null() && crate::ffi::build_parcheck_enabled() => {
            if let Some(t) = post_thread(post) {
                return usize::from(t.add_par_file(FileRef::from_addr(p2), a2 != 0));
            }
        }
        12 if !post.is_null() => {
            // the repair controller needs par-check support
            if a2 == 3 && !crate::ffi::build_parcheck_enabled() {
                return 0;
            }
            let nzb = post_nzb(post).cast_const();
            if a2 == 9 && !nzb.is_null() && unsafe { (*nzb).skipScriptProcessing } {
                return 0;
            }
            let job = JobHandle::new_post(a2, PostRef::from_addr(p1), 0, false);
            post_set(post, PostField::Thread, job.addr() as i64);
            if !job.start() {
                // no worker will finish this stage: let the scheduler clean up and retry it
                post_set(post, PostField::Thread, 0);
                post_set(post, PostField::Int(PostInt::Working), 0);
                unsafe { job.free(false) };
            }
        }
        _ => {}
    }
    0
}

/// Writes a collection's download counters (what host op 33 did), in the
/// same order: ints = file count, active downloads, full hash, filtered
/// hash, paused file count, remaining par count, total, success, failed,
/// current success, current failed articles, download seconds; i64s = size,
/// remaining, paused, success, current success, failed, current failed,
/// par size, par success, par current success, par failed, par current
/// failed, min time, max time, downloaded size, download start time.
///
/// # Safety
/// `p` is a live collection; the caller holds the queue lock.
pub(crate) unsafe fn host_set_stats(p: *mut NzbScalars, ints: &[i32; 12], i64s: &[i64; 16]) {
    let s = unsafe { &mut *p };
    s.fileCount = ints[0];
    set_active_downloads(s, ints[1], unix_now());
    s.fullContentHash = ints[2] as u32;
    s.filteredContentHash = ints[3] as u32;
    s.size = i64s[0];
    s.remainingSize = i64s[1];
    s.pausedFileCount = ints[4];
    s.pausedSize = i64s[2];
    s.successSize = i64s[3];
    s.currentSuccessSize = i64s[4];
    s.failedSize = i64s[5];
    s.currentFailedSize = i64s[6];
    s.parSize = i64s[7];
    s.parSuccessSize = i64s[8];
    s.parCurrentSuccessSize = i64s[9];
    s.parFailedSize = i64s[10];
    s.parCurrentFailedSize = i64s[11];
    s.remainingParCount = ints[5];
    s.totalArticles = ints[6];
    s.successArticles = ints[7];
    s.failedArticles = ints[8];
    s.currentSuccessArticles = ints[9];
    s.currentFailedArticles = ints[10];
    s.downloadSec = ints[11];
    s.minTime = i64s[12];
    s.maxTime = i64s[13];
    s.downloadedSize = i64s[14];
    s.downloadStartTime = i64s[15];
}

fn ptr_list_op(l: usize, op: c_int, a: i64, item: usize, order: *const usize, other: usize) -> i64 {
    unsafe {
        crate::qlist::nzbget_rs_ptrlist_op(
            l as *const crate::qlist::PtrList,
            op,
            a,
            item,
            order,
            other as *const crate::qlist::PtrList,
        )
    }
}

/// A collection's file list (slot 0).
pub(crate) fn file_list(p: *const NzbScalars) -> usize {
    slot(p, Slot::FileList)
}

/// Moves `src`'s files and download counters to `dst` (NzbInfo::MoveFileList).
unsafe fn move_file_list(dst: *mut NzbScalars, src: *const NzbScalars) {
    if std::ptr::eq(dst, src) {
        return;
    }
    let dl = file_list(dst);
    ptr_list_op(dl, 9, 0, 0, std::ptr::null(), 0);
    ptr_list_op(dl, 11, 0, 0, std::ptr::null(), file_list(src));
    for i in 0..ptr_list_op(dl, 0, 0, 0, std::ptr::null(), 0) {
        let f = ptr_list_op(dl, 1, i, 0, std::ptr::null(), 0);
        unsafe {
            crate::qfile::nzbget_rs_file_set(
                f as *const crate::qfile::File,
                crate::qfile::field::NZB,
                dst as i64,
            )
        };
    }
    let (d, s) = unsafe { (&mut *dst, &*src) };
    d.fullContentHash = s.fullContentHash;
    d.filteredContentHash = s.filteredContentHash;
    d.fileCount = s.fileCount;
    d.pausedFileCount = s.pausedFileCount;
    d.remainingParCount = s.remainingParCount;
    d.size = s.size;
    d.remainingSize = s.remainingSize;
    d.pausedSize = s.pausedSize;
    d.successSize = s.successSize;
    d.currentSuccessSize = s.currentSuccessSize;
    d.failedSize = s.failedSize;
    d.currentFailedSize = s.currentFailedSize;
    d.parSize = s.parSize;
    d.parSuccessSize = s.parSuccessSize;
    d.parCurrentSuccessSize = s.parCurrentSuccessSize;
    d.parFailedSize = s.parFailedSize;
    d.parCurrentFailedSize = s.parCurrentFailedSize;
    d.totalArticles = s.totalArticles;
    d.successArticles = s.successArticles;
    d.failedArticles = s.failedArticles;
    d.currentSuccessArticles = s.successArticles;
    d.currentFailedArticles = s.failedArticles;
    d.minTime = s.minTime;
    d.maxTime = s.maxTime;
}

fn scalars(nzb: NzbRef) -> *mut NzbScalars {
    nzb.addr() as *mut NzbScalars
}

/// Drops a file the list no longer holds (as the C++ unique_ptr did).
fn free_file(file: FileRef) {
    unsafe { crate::qfile::nzbget_rs_file_free(file.addr() as *mut crate::qfile::File) };
}

/// Takes `file` out of the download's list; whether it was there.
fn take_file(nzb: NzbRef, file: FileRef) -> bool {
    ptr_list_op(
        file_list(scalars(nzb)),
        6,
        0,
        file.addr(),
        std::ptr::null(),
        0,
    ) != 0
}

/// Appends `file` to the download's list.
fn append_file(nzb: NzbRef, file: FileRef) {
    ptr_list_op(
        file_list(scalars(nzb)),
        3,
        0,
        file.addr(),
        std::ptr::null(),
        0,
    );
}

/// The number of files queued for the download.
pub(crate) fn file_count(nzb: NzbRef) -> usize {
    if nzb.is_none() {
        return 0;
    }
    ptr_list_op(file_list(scalars(nzb)), 0, 0, 0, std::ptr::null(), 0) as usize
}

/// The `idx`-th queued file, or none.
pub(crate) fn file_at(nzb: NzbRef, idx: usize) -> FileRef {
    if nzb.is_none() {
        return FileRef::NONE;
    }
    let addr = ptr_list_op(
        file_list(scalars(nzb)),
        1,
        idx as i64,
        0,
        std::ptr::null(),
        0,
    );
    FileRef::from_addr(addr as usize)
}

/// Removes `file` from the download and frees it.
pub(crate) fn remove_file(nzb: NzbRef, file: FileRef) {
    if nzb.is_some() && take_file(nzb, file) {
        free_file(file);
    }
}

/// Removes `file`, tells the pre/post-processor about `aspect_action` on
/// queue `dq`, then frees it.
pub(crate) fn remove_file_and_notify(nzb: NzbRef, file: FileRef, aspect_action: i32, dq: usize) {
    if nzb.is_none() {
        return;
    }
    let released = take_file(nzb, file);
    crate::jobthread::nzbget_rs_prepost_queue_changed(aspect_action, dq, nzb.addr(), file.addr());
    if released {
        free_file(file);
    }
}

/// Moves `file` (split off a download) from `nzb` to `dest`, moving its
/// counts with it.
pub(crate) fn transfer_file(nzb: NzbRef, file: FileRef, dest: NzbRef) {
    if nzb.is_none() {
        return;
    }
    if take_file(nzb, file) {
        append_file(dest, file);
    }
    unsafe {
        crate::qfile::nzbget_rs_file_set(
            file.addr() as *const crate::qfile::File,
            crate::qfile::field::NZB,
            dest.addr() as i64,
        );
        crate::qfile::add_file_stats(
            nzb.addr(),
            file.addr() as *const crate::qfile::File,
            StatList::Current,
            StatOp::Subtract,
        );
        crate::qfile::add_file_stats(
            dest.addr(),
            file.addr() as *const crate::qfile::File,
            StatList::Current,
            StatOp::Add,
        );
    }
}

/// Moves every file, completed file and server stat of `src` into `nzb`
/// (merging two downloads).
pub(crate) fn merge_from(nzb: NzbRef, src: NzbRef) {
    if nzb.is_none() {
        return;
    }
    let (p, src) = (scalars(nzb), src.addr() as *const NzbScalars);
    let sl = file_list(src);
    let no: *const usize = std::ptr::null();
    for i in 0..ptr_list_op(sl, 0, 0, 0, no, 0) {
        let f = ptr_list_op(sl, 1, i, 0, no, 0) as usize;
        unsafe {
            crate::qfile::nzbget_rs_file_set(
                f as *const crate::qfile::File,
                crate::qfile::field::NZB,
                p as i64,
            )
        };
    }
    ptr_list_op(file_list(p), 11, 0, 0, no, sl);
    add_server_stats_from(nzb, NzbRef::from_addr(src as usize));
    if !src.is_null() && src != p.cast_const() {
        let taken = std::mem::take(&mut data(src).files().completed);
        data(p).files().completed.extend(taken);
    }
}

/// Puts the download's files in `order` (files not named keep their place
/// after them, as the list op does).
pub(crate) fn reorder_files(nzb: NzbRef, order: &[FileRef]) {
    if nzb.is_none() || order.is_empty() {
        return;
    }
    // FileRef is a transparent address, so the slice is a list of addresses.
    let addrs = order.as_ptr().cast::<usize>();
    ptr_list_op(file_list(scalars(nzb)), 10, order.len() as i64, 0, addrs, 0);
}

/// Replaces the download's files, completed files and stats with those of
/// `src` (a redownload).
pub(crate) fn replace_files_from(nzb: NzbRef, src: NzbRef) {
    if nzb.is_none() {
        return;
    }
    let p = scalars(nzb);
    {
        let mut f = data(p).files();
        f.completed.clear();
        f.jobs.clear();
    }
    if let Some(d) = nzb_data(nzb) {
        d.coll().stats.iter_mut().for_each(Vec::clear);
    }
    unsafe { move_file_list(p, src.addr() as *const NzbScalars) };
}

/// Erases the `idx`-th completed file.
pub(crate) fn erase_completed_file(nzb: NzbRef, idx: usize) {
    if let Some(d) = nzb_data(nzb) {
        let mut f = d.files();
        if idx < f.completed.len() {
            f.completed.remove(idx);
        }
    }
}

/// Appends a requeued `file` to the download.
pub(crate) fn push_file(nzb: NzbRef, file: FileRef) {
    if nzb.is_some() {
        append_file(nzb, file);
    }
}

fn completed_snap(c: &Completed) -> crate::queuesubsys_host::CompletedFileSnap {
    crate::queuesubsys_host::CompletedFileSnap {
        id: c.id,
        status: c.status,
        par_file: c.par,
        crc: c.crc,
        filename: c.filename.to_bytes().to_vec(),
        origname: c.origname.to_bytes().to_vec(),
        hash16k: c.hash16k.to_bytes().to_vec(),
        par_set_id: c.par_set_id.to_bytes().to_vec(),
    }
}

/// The number of completed files.
pub(crate) fn completed_file_count(nzb: NzbRef) -> usize {
    nzb_data(nzb).map_or(0, |d| d.files().completed.len())
}

/// The `idx`-th completed file, or `None` past the end.
pub(crate) fn completed_file(
    nzb: NzbRef,
    idx: usize,
) -> Option<crate::queuesubsys_host::CompletedFileSnap> {
    nzb_data(nzb).and_then(|d| d.files().completed.get(idx).map(completed_snap))
}

/// Every completed file, in order.
pub(crate) fn completed_files(nzb: NzbRef) -> Vec<crate::queuesubsys_host::CompletedFileSnap> {
    nzb_data(nzb).map_or_else(Vec::new, |d| {
        d.files().completed.iter().map(completed_snap).collect()
    })
}

/// Appends a completed file, raising the file id floor
/// (NzbInfo::AddCompletedFile). Text stops at the first NUL.
pub(crate) fn push_completed_file(nzb: NzbRef, cf: &crate::queuesubsys_host::CompletedFileSnap) {
    let Some(d) = nzb_data(nzb) else {
        return;
    };
    crate::idgen::FILE_IDS.raise_floor(cf.id);
    d.files().completed.push(Completed {
        id: cf.id,
        status: cf.status,
        crc: cf.crc,
        par: cf.par_file,
        filename: c_string(&cf.filename),
        origname: c_string(&cf.origname),
        hash16k: c_string(&cf.hash16k),
        par_set_id: c_string(&cf.par_set_id),
    });
}

/// Records queued `file` as completed under `filename` with `status` and
/// `crc`, keeping its id, par flag, original name, 16k hash and par set.
pub(crate) fn complete_file(nzb: NzbRef, file: FileRef, filename: &[u8], status: i32, crc: u32) {
    use crate::qfile::{field, nzbget_rs_file_get, nzbget_rs_file_text, text, File};
    if nzb.is_none() || file.is_none() {
        return;
    }
    let f = file.addr() as *const File;
    let t = |id| {
        cstr(unsafe { nzbget_rs_file_text(f, id) })
            .to_bytes()
            .to_vec()
    };
    let cf = crate::queuesubsys_host::CompletedFileSnap {
        id: unsafe { nzbget_rs_file_get(f, field::ID) } as i32,
        status,
        par_file: unsafe { nzbget_rs_file_get(f, field::PAR_FILE) } != 0,
        crc,
        filename: filename.to_vec(),
        origname: t(text::ORIGNAME),
        hash16k: t(text::HASH16K),
        par_set_id: t(text::PAR_SET_ID),
    };
    push_completed_file(nzb, &cf);
}

/// Renames the `idx`-th completed file to `filename`, and sets its original
/// name when `origname` is given.
pub(crate) fn rename_completed(nzb: NzbRef, idx: usize, filename: &[u8], origname: Option<&[u8]>) {
    let Some(d) = nzb_data(nzb) else {
        return;
    };
    if let Some(c) = d.files().completed.get_mut(idx) {
        if let Some(o) = origname {
            c.origname = c_string(o);
        }
        c.filename = c_string(filename);
    }
}

type ByteRange = crate::dupefallback::NzbgetRsByteRange;

fn hole_pairs(holes: &[ByteRange]) -> Vec<[i64; 2]> {
    holes.iter().map(|r| [r.offset, r.size]).collect()
}

/// Updates the status of the completed file with id `file_id`: mode 1
/// marks it successful (true when it was not), mode 2 turns a failure into
/// a partial success.
pub(crate) fn update_completed_status(nzb: NzbRef, file_id: i32, mode: i32) -> bool {
    let Some(d) = nzb_data(nzb) else {
        return false;
    };
    let mut f = d.files();
    if let Some(c) = f.completed.iter_mut().find(|c| c.id == file_id) {
        if mode == 1 && c.status != 1 {
            c.status = 1;
            return true;
        }
        if mode == 2 && c.status == 3 {
            c.status = 2;
        }
    }
    false
}

/// Queues a stream-repair job (not yet tried by the live pass).
pub(crate) fn push_repair_job(
    nzb: NzbRef,
    job: &crate::queuesubsys_host::StreamRepairJobDetailSnap,
) {
    if let Some(d) = nzb_data(nzb) {
        d.files().jobs.push(RepairJob {
            file_id: job.file_id,
            filename: c_string(&job.filename),
            decoded_size: job.decoded_file_size,
            failed_size: job.failed_size,
            missed_size: job.missed_size,
            failed_articles: job.failed_articles,
            par: job.par_file,
            holes: hole_pairs(&job.holes),
            steps_hash: job.steps_hash,
            live_attempted: false,
        });
    }
}

/// The number of stream-repair jobs.
pub(crate) fn repair_job_count(nzb: NzbRef) -> usize {
    nzb_data(nzb).map_or(0, |d| d.files().jobs.len())
}

/// A summary of every stream-repair job, in order.
pub(crate) fn repair_jobs(nzb: NzbRef) -> Vec<crate::queuesubsys_host::StreamRepairJobSnap> {
    nzb_data(nzb).map_or_else(Vec::new, |d| {
        d.files()
            .jobs
            .iter()
            .map(|j| crate::queuesubsys_host::StreamRepairJobSnap {
                file_id: j.file_id,
                decoded_file_size: j.decoded_size,
                holes_total_size: j.holes.iter().map(|r| r[1]).sum(),
                failed_plus_missed_size: j.failed_size + j.missed_size,
                filename: j.filename.to_bytes().to_vec(),
            })
            .collect()
    })
}

/// The `idx`-th stream-repair job in full, or `None` past the end.
pub(crate) fn repair_job(
    nzb: NzbRef,
    idx: usize,
) -> Option<crate::queuesubsys_host::StreamRepairJobDetailSnap> {
    let d = nzb_data(nzb)?;
    let f = d.files();
    let j = f.jobs.get(idx)?;
    Some(crate::queuesubsys_host::StreamRepairJobDetailSnap {
        file_id: j.file_id,
        failed_articles: j.failed_articles,
        par_file: j.par,
        decoded_file_size: j.decoded_size,
        failed_size: j.failed_size,
        missed_size: j.missed_size,
        steps_hash: j.steps_hash,
        filename: j.filename.to_bytes().to_vec(),
        holes: j
            .holes
            .iter()
            .map(|r| ByteRange {
                offset: r[0],
                size: r[1],
            })
            .collect(),
    })
}

/// Marks the `idx`-th job as tried by the live pass; whether it already was.
pub(crate) fn mark_repair_job_live_attempted(nzb: NzbRef, idx: usize) -> bool {
    nzb_data(nzb).is_some_and(|d| {
        d.files()
            .jobs
            .get_mut(idx)
            .is_some_and(|j| std::mem::replace(&mut j.live_attempted, true))
    })
}

/// Removes the `idx`-th job, marking the download changed when asked.
pub(crate) fn erase_repair_job(nzb: NzbRef, idx: usize, set_changed: bool) {
    let Some(d) = nzb_data(nzb) else {
        return;
    };
    {
        let mut f = d.files();
        if idx < f.jobs.len() {
            f.jobs.remove(idx);
        }
    }
    if set_changed {
        unsafe { (*scalars(nzb)).changed = true };
    }
}

/// Replaces the `idx`-th job's holes and, when `decoded_size >= 0`, its
/// decoded size.
pub(crate) fn update_repair_job_holes(
    nzb: NzbRef,
    idx: usize,
    decoded_size: i64,
    holes: &[ByteRange],
) {
    let Some(d) = nzb_data(nzb) else {
        return;
    };
    if let Some(j) = d.files().jobs.get_mut(idx) {
        if decoded_size >= 0 {
            j.decoded_size = decoded_size;
        }
        j.holes = hole_pairs(holes);
    }
}

/// A file parsed from an NZB, as [`add_parsed_file`] queues it. Pointers are
/// null or C strings; the article slices have one entry per article.
pub(crate) struct ParsedFile<'a> {
    /// 0 takes a fresh id.
    pub id: i32,
    pub subject: *const c_char,
    pub filename: *const c_char,
    pub time: i32,
    pub size: i64,
    pub remaining_size: i64,
    pub missed_size: i64,
    pub total_articles: i32,
    pub missed_articles: i32,
    pub par_file: bool,
    pub groups: &'a [*const c_char],
    pub article_parts: &'a [i32],
    pub article_sizes: &'a [i64],
    pub article_msg_ids: &'a [*const c_char],
}

/// Queues a new file parsed from an NZB with its groups and articles.
pub(crate) fn add_parsed_file(nzb: NzbRef, pf: &ParsedFile<'_>) -> FileRef {
    use crate::qfile::{field, nzbget_rs_file_set, nzbget_rs_file_set_text, text};
    if nzb.is_none() {
        return FileRef::NONE;
    }
    let or_empty = |s: *const c_char| if s.is_null() { c"".as_ptr() } else { s };
    let f = crate::qfile::nzbget_rs_file_new(-1);
    let set = |id, v: i64| unsafe { nzbget_rs_file_set(f, id, v) };
    let id = if pf.id != 0 {
        pf.id
    } else {
        crate::idgen::FILE_IDS.next()
    };
    set(field::ID, i64::from(id));
    set(field::NZB, nzb.addr() as i64);
    unsafe {
        nzbget_rs_file_set_text(f, text::SUBJECT, or_empty(pf.subject));
        nzbget_rs_file_set_text(f, text::FILENAME, or_empty(pf.filename));
    }
    set(field::TIME, i64::from(pf.time));
    set(field::SIZE, pf.size);
    set(field::REMAINING_SIZE, pf.remaining_size);
    set(field::MISSED_SIZE, pf.missed_size);
    set(field::TOTAL_ARTICLES, i64::from(pf.total_articles));
    set(field::MISSED_ARTICLES, i64::from(pf.missed_articles));
    set(field::PAR_FILE, i64::from(pf.par_file));
    for &name in pf.groups {
        unsafe {
            crate::qfile::nzbget_rs_file_op(
                f,
                2,
                0,
                or_empty(name),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
    }
    let addr_or_null = |empty: bool, addr: usize| if empty { 0 } else { addr as i64 };
    let mut art_args = [
        addr_or_null(
            pf.article_parts.is_empty(),
            pf.article_parts.as_ptr() as usize,
        ),
        addr_or_null(
            pf.article_sizes.is_empty(),
            pf.article_sizes.as_ptr() as usize,
        ),
    ];
    let msg_ids = if pf.article_msg_ids.is_empty() {
        std::ptr::null_mut()
    } else {
        pf.article_msg_ids.as_ptr().cast_mut()
    };
    unsafe {
        crate::qarticle::nzbget_rs_article_host(
            crate::qfile::nzbget_rs_file_articles(f),
            std::ptr::null_mut(),
            105,
            pf.article_parts.len() as c_int,
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
            art_args.as_mut_ptr(),
            msg_ids,
        )
    };
    let file = FileRef::from_addr(f as usize);
    append_file(nzb, file);
    file
}

/// Queues an empty file record with a saved id (disk state restore); `time`
/// is set only when given.
pub(crate) fn add_stub_file(
    nzb: NzbRef,
    id: i32,
    paused: bool,
    time: Option<i32>,
    extra_priority: bool,
) {
    use crate::qfile::{field, nzbget_rs_file_set};
    if nzb.is_none() {
        return;
    }
    let f = crate::qfile::nzbget_rs_file_new(-1);
    let set = |field_id, v: i64| unsafe { nzbget_rs_file_set(f, field_id, v) };
    // new FileInfo() takes a generated id before SetId replaces it
    set(field::ID, i64::from(crate::idgen::FILE_IDS.next()));
    set(field::ID, i64::from(id));
    set(field::PAUSED, i64::from(paused));
    if let Some(t) = time {
        set(field::TIME, i64::from(t));
    }
    set(field::EXTRA_PRIORITY, i64::from(extra_priority));
    set(field::NZB, nzb.addr() as i64);
    append_file(nzb, FileRef::from_addr(f as usize));
}

/// NzbInfo::SetId: the id, raising the id floor.
fn set_id(s: &mut NzbScalars, id: c_int) {
    s.id = id;
    crate::idgen::NZB_IDS.raise_floor(id);
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// NzbInfo::SetActiveDownloads: accounts download time when downloads start
/// or stop.
fn set_active_downloads(s: &mut NzbScalars, active: c_int, now: i64) {
    let (sec, start_time, start_sec, changed) = crate::downloadinfo::calc_active_downloads_sec(
        s.activeDownloads,
        active,
        s.kind == 0,
        now,
        s.downloadSec,
        s.downloadStartTime,
        s.downloadStartSec,
    );
    s.downloadSec = sec;
    s.downloadStartTime = start_time;
    s.downloadStartSec = start_sec;
    if changed {
        s.changed = true;
    }
    s.activeDownloads = active;
}

/// Text field `id` as bytes (empty when unset).
fn text_bytes(p: *const NzbScalars, id: c_int) -> Vec<u8> {
    let t = unsafe { nzbget_rs_nzb_text(p, id) };
    if t.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(t) }.to_bytes().to_vec()
    }
}

fn text_is_set(p: *const NzbScalars, id: c_int) -> bool {
    !unsafe { nzbget_rs_nzb_text(p, id) }.is_null()
}

/// Sets text field `id` to `bytes` (up to an embedded NUL, as C++ did).
fn set_text_bytes(p: *const NzbScalars, id: c_int, bytes: &[u8]) {
    unsafe { nzbget_rs_nzb_set_text(p, id, bytes.as_ptr().cast(), bytes.len() as isize) };
}

/// The destination directory of the collection's category, when it names
/// one (empty otherwise).
fn category_dest_dir(opts: Option<&crate::options::NzbgetRsOptions>, category: &[u8]) -> Vec<u8> {
    let (Some(opts), false) = (opts, category.is_empty()) else {
        return Vec::new();
    };
    let Ok(name) = CString::new(category) else {
        return Vec::new();
    };
    opts.find_category(&name, false)
        .map(|c| c.dest_dir.to_bytes().to_vec())
        .unwrap_or_default()
}

/// NzbInfo::BuildFinalDirName.
unsafe fn final_dir_name(p: *const NzbScalars) -> Vec<u8> {
    let opts = crate::options::active_options();
    let o = opts.as_deref();
    let category = text_bytes(p, 5);
    crate::downloadinfo::build_final_dir_name(
        o.map_or(&[][..], |o| o.str_bytes(OptStr::DestDir)),
        &category_dest_dir(o, &category),
        &category,
        o.is_some_and(|o| o.scalars().append_category_dir != 0),
        &text_bytes(p, 2),
        unsafe { (*p).id },
    )
}

/// NzbInfo::BuildDestDirName: sets the destination directory.
unsafe fn build_dest_dir_name(p: *mut NzbScalars) {
    let opts = crate::options::active_options();
    let o = opts.as_deref();
    let inter_dir = o.map_or(&[][..], |o| o.str_bytes(OptStr::InterDir));
    let category = text_bytes(p, 5);
    let category_dest = if inter_dir.is_empty() {
        category_dest_dir(o, &category)
    } else {
        Vec::new()
    };
    let dir = crate::downloadinfo::build_dest_dir_name(
        inter_dir,
        o.map_or(&[][..], |o| o.str_bytes(OptStr::DestDir)),
        &category_dest,
        &category,
        o.is_some_and(|o| o.scalars().append_category_dir != 0),
        &text_bytes(p, 2),
        unsafe { (*p).id },
    );
    set_text_bytes(p, 3, &dir);
}

/// NzbInfo::SetFilename: names the collection from the file name when it
/// has no name yet, or had no file name.
unsafe fn set_filename(p: *mut NzbScalars, filename: *const c_char) {
    let had_filename = !text_bytes(p, 1).is_empty();
    unsafe { nzbget_rs_nzb_set_text(p, 1, filename, -1) };
    if (!text_is_set(p, 2) || !had_filename) && !cstr(filename).is_empty() {
        let mut nice = crate::downloadinfo::make_nice_nzb_name(&text_bytes(p, 1), true);
        if nice.is_empty() {
            nice = format!("nzb_{}", unsafe { (*p).id }).into_bytes();
        }
        set_text_bytes(p, 2, &nice);
    }
}

/// NzbInfo::SetUrl: names the collection from the URL when it has no name.
unsafe fn set_url(p: *mut NzbScalars, url: *const c_char) {
    unsafe { nzbget_rs_nzb_set_text(p, 0, url, -1) };
    if !text_is_set(p, 2) {
        let url = (!url.is_null()).then(|| unsafe { CStr::from_ptr(url) });
        let nice = crate::downloadinfo::make_nice_url_name(url, &text_bytes(p, 1));
        set_text_bytes(p, 2, &nice);
    }
}

fn files_of(p: *const NzbScalars) -> Vec<*const crate::qfile::File> {
    let fl = file_list(p);
    if fl == 0 {
        return Vec::new();
    }
    crate::qlist::items(fl as *const crate::qlist::PtrList)
        .into_iter()
        .map(|f| f as *const crate::qfile::File)
        .collect()
}

fn file_get(f: *const crate::qfile::File, id: c_int) -> i64 {
    unsafe { crate::qfile::nzbget_rs_file_get(f, id) }
}

/// NzbInfo::UpdateMinMaxTime over the file list.
unsafe fn update_min_max_time(p: *mut NzbScalars) {
    let times: Vec<i64> = files_of(p)
        .into_iter()
        .map(|f| file_get(f, crate::qfile::field::TIME))
        .collect();
    let (min_t, max_t) = crate::downloadinfo::calc_min_max_time(&times);
    let s = unsafe { &mut *p };
    s.minTime = min_t;
    s.maxTime = max_t;
}

/// NzbInfo::UpdateCompletedStats: a file finished downloading.
unsafe fn update_completed_stats(p: *mut NzbScalars, f: *const crate::qfile::File) {
    use crate::qfile::field;
    let g = |id| file_get(f, id);
    {
        let s = unsafe { &mut *p };
        crate::downloadinfo::update_completed_stats(
            g(field::SUCCESS_SIZE),
            g(field::FAILED_SIZE),
            g(field::FAILED_ARTICLES) as i32,
            g(field::SUCCESS_ARTICLES) as i32,
            g(field::EXTRA_PRIORITY) != 0,
            g(field::PAR_FILE) != 0,
            g(field::PAUSED) != 0,
            &mut s.successSize,
            &mut s.failedSize,
            &mut s.failedArticles,
            &mut s.successArticles,
            &mut s.extraPriority,
            &mut s.parSuccessSize,
            &mut s.parFailedSize,
            &mut s.remainingParCount,
            &mut s.pausedFileCount,
        );
    }
    unsafe { crate::qfile::add_file_stats(p as usize, f, StatList::Total, StatOp::Add) };
}

/// NzbInfo::UpdateDeletedStats: a file left the queue undownloaded.
unsafe fn update_deleted_stats(p: *mut NzbScalars, f: *const crate::qfile::File) {
    use crate::qfile::field;
    let g = |id| file_get(f, id);
    {
        let s = unsafe { &mut *p };
        crate::downloadinfo::update_deleted_stats(
            g(field::SIZE),
            g(field::SUCCESS_SIZE),
            g(field::FAILED_SIZE),
            g(field::MISSED_SIZE),
            g(field::TOTAL_ARTICLES) as i32,
            g(field::SUCCESS_ARTICLES) as i32,
            g(field::FAILED_ARTICLES) as i32,
            g(field::MISSED_ARTICLES) as i32,
            g(field::REMAINING_SIZE),
            g(field::EXTRA_PRIORITY) != 0,
            g(field::PAR_FILE) != 0,
            g(field::PAUSED) != 0,
            &mut s.fileCount,
            &mut s.size,
            &mut s.currentSuccessSize,
            &mut s.failedSize,
            &mut s.failedArticles,
            &mut s.currentFailedSize,
            &mut s.totalArticles,
            &mut s.currentSuccessArticles,
            &mut s.currentFailedArticles,
            &mut s.remainingSize,
            &mut s.extraPriority,
            &mut s.remainingParCount,
            &mut s.parSize,
            &mut s.parCurrentSuccessSize,
            &mut s.parFailedSize,
            &mut s.parCurrentFailedSize,
            &mut s.pausedFileCount,
            &mut s.pausedSize,
        );
    }
    unsafe { crate::qfile::add_file_stats(p as usize, f, StatList::Current, StatOp::Subtract) };
}

/// NzbInfo::UpdateCurrentStats: recomputes the current-session counters
/// from the totals and the files still queued.
unsafe fn update_current_stats(p: *mut NzbScalars) {
    use crate::qfile::field;
    let mut acc = {
        let s = unsafe { &*p };
        crate::downloadinfo::RsDownloadCurrentStatsAcc {
            current_success_articles: s.successArticles,
            current_failed_articles: s.failedArticles,
            current_success_size: s.successSize,
            current_failed_size: s.failedSize,
            par_current_success_size: s.parSuccessSize,
            par_current_failed_size: s.parFailedSize,
            ..Default::default()
        }
    };
    sync_current_server_stats(NzbRef::from_addr(p as usize));
    for f in files_of(p) {
        let g = |id| file_get(f, id);
        crate::downloadinfo::update_current_stats_step(
            &mut acc,
            g(field::REMAINING_SIZE),
            g(field::SUCCESS_ARTICLES) as i32,
            g(field::FAILED_ARTICLES) as i32,
            g(field::SUCCESS_SIZE),
            g(field::FAILED_SIZE),
            g(field::EXTRA_PRIORITY) != 0,
            g(field::PAUSED) != 0,
            g(field::PAR_FILE) != 0,
        );
        unsafe { crate::qfile::add_file_stats(p as usize, f, StatList::Current, StatOp::Add) };
    }
    let s = unsafe { &mut *p };
    s.pausedFileCount = acc.paused_file_count;
    s.remainingParCount = acc.remaining_par_count;
    s.remainingSize = acc.remaining_size;
    s.pausedSize = acc.paused_size;
    s.currentSuccessArticles = acc.current_success_articles;
    s.currentFailedArticles = acc.current_failed_articles;
    s.currentSuccessSize = acc.current_success_size;
    s.currentFailedSize = acc.current_failed_size;
    s.parCurrentSuccessSize = acc.par_current_success_size;
    s.parCurrentFailedSize = acc.par_current_failed_size;
    s.extraPriority = acc.extra_priority;
}

/// Whether a file still downloading has its output file open (host op 31
/// sub-op 22): checked again under the file's output lock.
unsafe fn has_active_output(p: *const NzbScalars) -> bool {
    use crate::qfile::{field, nzbget_rs_file_op, nzbget_rs_file_text, text};
    for f in files_of(p) {
        let out = unsafe { nzbget_rs_file_text(f, text::OUTPUT_FILENAME) };
        let has_out = !out.is_null() && unsafe { *out } != 0;
        if file_get(f, field::OUTPUT_INITIALIZED) != 0
            && has_out
            && file_get(f, field::ACTIVE_DOWNLOADS) > 0
        {
            let ns = std::ptr::null();
            let taken = unsafe {
                nzbget_rs_file_op(f, 20, 0, ns, std::ptr::null_mut(), std::ptr::null_mut())
            } != 0;
            let initialized = file_get(f, field::OUTPUT_INITIALIZED) != 0;
            if taken {
                unsafe {
                    nzbget_rs_file_op(f, 21, 0, ns, std::ptr::null_mut(), std::ptr::null_mut())
                };
            }
            if initialized {
                return true;
            }
        }
    }
    false
}

fn calc_health(s: &NzbScalars) -> i32 {
    crate::downloadinfo::calc_health(
        s.size,
        s.parSize,
        s.currentFailedSize,
        s.parCurrentFailedSize,
    )
}

fn calc_critical_health(s: &NzbScalars, allow_estimation: bool) -> i32 {
    crate::downloadinfo::calc_critical_health(
        s.size,
        s.parSize,
        s.parCurrentFailedSize,
        allow_estimation,
    )
}

/// NzbInfo::IsDownloadCompleted over the file list, ignoring paused par files and strictly,
/// from one read of the list. (Strict implies the other, so it is only checked then.)
fn download_completed(p: *const NzbScalars) -> (bool, bool) {
    use crate::qfile::field;
    let active = unsafe { (*p).activeDownloads };
    if active != 0 {
        return (false, false);
    }
    let files = files_of(p);
    let check = |ignore_paused_pars| {
        crate::downloadinfo::is_download_completed_with(active, ignore_paused_pars, files.len(), |i| {
            let f = files[i];
            (
                file_get(f, field::PAUSED) != 0,
                file_get(f, field::PAR_FILE) != 0,
                file_get(f, field::DELETED) != 0,
            )
        })
    };
    let lax = check(true);
    (lax, lax && check(false))
}

/// Health check option value `hcDupe`.
const HEALTH_CHECK_DUPE: i32 = 4;
/// Priority at and above which a collection is forced.
const FORCE_PRIORITY: i32 = 900;

/// Which text status a collection snapshot carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextStatus {
    /// None: the snapshot's text status is empty.
    Off,
    /// As if the scripts succeeded.
    IgnoreScript,
    /// Including the scripts' total status.
    WithScript,
}

/// The fields the next-article pick reads first (priority, extra priority, paused file
/// count), without a whole snapshot. `None` for a collection that is gone.
pub(crate) fn nzb_pick_fields(nzb: NzbRef) -> Option<(i32, bool, i32)> {
    nzb_data(nzb)?;
    let s = unsafe { &*scalars(nzb).cast_const() };
    Some((s.priority, s.extraPriority > 0, s.pausedFileCount))
}

/// A collection's duplicate key alone (empty when unset or gone).
pub(crate) fn nzb_dupe_key(nzb: NzbRef) -> Vec<u8> {
    if nzb_data(nzb).is_none() {
        return Vec::new();
    }
    text_bytes(scalars(nzb).cast_const(), 7)
}

/// A copy of a collection's fields and the values computed from them.
/// A none handle gives the default snapshot. The caller holds the queue
/// lock.
pub(crate) fn nzb_snapshot(
    nzb: NzbRef,
    text_status: TextStatus,
) -> crate::queuesubsys_host::NzbSnap {
    use crate::status::{
        CleanupStatus, DirectRenameStatus, DirectUnpackStatus, DupeMode, MarkStatus, MoveStatus,
        NzbParStatus, PostStage, UnpackStatus, UrlStatus,
    };
    if nzb_data(nzb).is_none() {
        return crate::queuesubsys_host::NzbSnap::default();
    }
    let p = scalars(nzb).cast_const();
    let s = unsafe { &*p };
    let script_total = script_total_status(nzb);
    let post = slot(p, Slot::Post) as *const Post;
    let post_flag = |f| !post.is_null() && post_get(post, PostField::Int(f)) != 0;
    let unpack_thread = slot(p, Slot::UnpackJob);
    let live_repair_thread = slot(p, Slot::LiveRepairJob);
    let message_count = slot(p, Slot::MessageCount) as isize as c_int;
    let health = calc_health(s);
    let critical_est = calc_critical_health(s, true);
    let critical_exact = calc_critical_health(s, false);
    let completed = download_completed(p);
    let health_check_dupe = crate::options::active_options()
        .is_some_and(|o| o.scalars().health_check == HEALTH_CHECK_DUPE);
    let is_dupe_success = crate::downloadinfo::is_dupe_success(
        MarkStatus::from(s.markStatus),
        DeleteStatus::from(s.deleteStatus),
        NzbParStatus::from(s.parStatus),
        UnpackStatus::from(s.unpackStatus),
        UrlStatus::from(s.urlStatus),
        health,
        critical_est,
        critical_exact,
        health_check_dupe,
    );
    let text_status = match text_status {
        TextStatus::Off => Vec::new(),
        TextStatus::IgnoreScript | TextStatus::WithScript => {
            let status = crate::downloadinfo::make_text_status_cstr(
                s.kind,
                MarkStatus::from(s.markStatus),
                DeleteStatus::from(s.deleteStatus),
                NzbParStatus::from(s.parStatus),
                UnpackStatus::from(s.unpackStatus),
                MoveStatus::from(s.moveStatus),
                UrlStatus::from(s.urlStatus),
                if text_status == TextStatus::IgnoreScript {
                    ScriptStatus::Success
                } else {
                    script_total
                },
                health,
                critical_exact,
            );
            status.strip_suffix(b"\0").unwrap_or(status).to_vec()
        }
    };
    crate::queuesubsys_host::NzbSnap {
        ptr: nzb,
        post_info_ptr: PostRef::from_addr(post as usize),
        id: s.id,
        kind: s.kind,
        dupe_mode: DupeMode::from(s.dupeMode),
        dupe_score: s.dupeScore,
        dupe_hint: s.dupeHint,
        delete_status: DeleteStatus::from(s.deleteStatus),
        mark_status: MarkStatus::from(s.markStatus),
        par_status: NzbParStatus::from(s.parStatus),
        direct_rename_status: DirectRenameStatus::from(s.directRenameStatus),
        direct_unpack_status: DirectUnpackStatus::from(s.directUnpackStatus),
        dupe_par_defer_state: s.dupeParDeferState,
        deleting: s.deleting,
        parking: s.parking,
        health_paused: s.healthPaused,
        force_priority: s.priority >= FORCE_PRIORITY,
        has_extra_priority: s.extraPriority > 0,
        many_dupe_files: s.manyDupeFiles,
        has_desired_server: s.desiredServerId > 0,
        desired_server_id: s.desiredServerId,
        priority: s.priority,
        feed_id: s.feedId,
        changed: s.changed,
        all_first: s.allFirst,
        dead_pick_probed: s.deadPickProbed,
        dupe_no_par_noted: s.dupeNoParNoted,
        has_post_info: !post.is_null(),
        post_working: post_flag(PostInt::Working),
        post_stage: if post.is_null() {
            PostStage::default()
        } else {
            PostStage::from(post_get(post, PostField::Int(PostInt::Stage)) as c_int)
        },
        has_live_repair_thread: live_repair_thread != 0,
        live_repair_thread_ptr: live_repair_thread,
        has_unpack_thread: unpack_thread != 0,
        has_stream_repair_jobs: repair_job_count(nzb) > 0,
        url_status: UrlStatus::from(s.urlStatus),
        par_sec: s.parSec,
        repair_sec: s.repairSec,
        unpack_sec: s.unpackSec,
        live_unpack_sec: s.liveUnpackSec,
        live_par_sec: s.liveParSec,
        reprocess: s.reprocess,
        extra_par_blocks: s.extraParBlocks,
        dupe_recovered_holes: s.dupeRecoveredHoles,
        script_total_status: script_total,
        parked_file_count: s.parkedFileCount,
        delete_paused: s.deletePaused,
        auto_category: s.autoCategory,
        add_url_paused: s.addUrlPaused,
        is_dupe_success,
        health,
        critical_health_opt: critical_est,
        critical_health_exact: critical_exact,
        is_download_completed: completed.0,
        is_download_completed_strict: completed.1,
        skip_disk_write: s.skipDiskWrite,
        waiting_par: s.waitingPar,
        loading_par: s.loadingPar,
        unpack_cleaned_up_disk: s.unpackCleanedUpDisk,
        skip_script_processing: s.skipScriptProcessing,
        avoid_history: s.avoidHistory,
        cleanup_disk: s.cleanupDisk,
        unpack_tried: s.unpackTried,
        par_full: s.parFull,
        force_par_full: post_flag(PostInt::ForceParFull),
        force_repair: post_flag(PostInt::ForceRepair),
        par_repaired: s.parRepaired,
        last_unpack_status: UnpackStatus::from(s.lastUnpackStatus),
        message_count,
        cached_message_count: s.cachedMessageCount,
        post_total_sec: s.postTotalSec,
        par_rename_status: s.parRenameStatus,
        rar_rename_status: s.rarRenameStatus,
        unpack_status: UnpackStatus::from(s.unpackStatus),
        cleanup_status: CleanupStatus::from(s.cleanupStatus),
        move_status: MoveStatus::from(s.moveStatus),
        post_download_renaming_status: s.postDownloadRenamingStatus,
        post_unpack_renaming_status: s.postUnpackRenamingStatus,
        file_count: s.fileCount,
        active_downloads: s.activeDownloads,
        paused_file_count: s.pausedFileCount,
        remaining_par_count: s.remainingParCount,
        total_articles: s.totalArticles,
        success_articles: s.successArticles,
        failed_articles: s.failedArticles,
        current_success_articles: s.currentSuccessArticles,
        current_failed_articles: s.currentFailedArticles,
        dupe_recovered_articles: s.dupeRecoveredArticles,
        dupe_attempted_articles: s.dupeAttemptedArticles,
        dupe_unsourced_articles: s.dupeUnsourcedArticles,
        dupe_failed_run: s.dupeFailedRun,
        dupe_failover_checked: s.dupeFailoverChecked,
        download_sec: s.downloadSec,
        full_content_hash: s.fullContentHash,
        filtered_content_hash: s.filteredContentHash,
        size: s.size,
        remaining_size: s.remainingSize,
        paused_size: s.pausedSize,
        success_size: s.successSize,
        current_success_size: s.currentSuccessSize,
        failed_size: s.failedSize,
        current_failed_size: s.currentFailedSize,
        par_size: s.parSize,
        par_success_size: s.parSuccessSize,
        par_current_success_size: s.parCurrentSuccessSize,
        par_failed_size: s.parFailedSize,
        par_current_failed_size: s.parCurrentFailedSize,
        downloaded_size: s.downloadedSize,
        min_time: s.minTime,
        max_time: s.maxTime,
        download_start_time: s.downloadStartTime,
        dupe_recovered_bytes: s.dupeRecoveredBytes,
        name: text_bytes(p, 2),
        filename: text_bytes(p, 1),
        dest_dir: text_bytes(p, 3),
        final_dir: text_bytes(p, 4),
        category: text_bytes(p, 5),
        dupe_key: text_bytes(p, 7),
        queued_filename: text_bytes(p, 6),
        text_status,
        url: text_bytes(p, 0),
    }
}

/// Appends `new` (kind, text) messages to collection `p`'s log at `time`,
/// with ids from its idMessageGen, keeping at most `limit` when > 0.
/// cachedMessageCount follows. Returns the count.
fn log_messages(
    p: *const NzbScalars,
    time: i64,
    limit: i32,
    new: impl IntoIterator<Item = (i32, CString)>,
) -> usize {
    let mut m = data(p).msgs.lock().unwrap_or_else(|e| e.into_inner());
    // the scalars are C++-owned plain memory; these two fields change only
    // under this lock
    let sp = p.cast_mut();
    for (kind, text) in new {
        let id = unsafe { (*sp).idMessageGen.wrapping_add(1) };
        unsafe { (*sp).idMessageGen = id };
        m.push_back((id as u32, kind, time, text));
        while limit > 0 && m.len() > limit as usize {
            m.pop_front();
        }
    }
    unsafe { (*sp).cachedMessageCount = m.len() as i32 };
    m.len()
}

/// Keeps message `text` of `kind` in a collection's log (the caller
/// prints it), at `time`, keeping at most `limit` when > 0. Returns the
/// count. Read them with [`cached_messages`].
pub(crate) fn add_message(nzb: NzbRef, kind: i32, text: CString, time: i64, limit: i32) -> usize {
    nzb_data(nzb).map_or(0, |_| {
        log_messages(scalars(nzb), time, limit, [(kind, text)])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_start_empty_except_name() {
        let p = nzbget_rs_nzb_scalars_new();
        unsafe {
            assert!(nzbget_rs_nzb_text(p, 2).is_null());
            assert_eq!(CStr::from_ptr(nzbget_rs_nzb_text(p, 0)).to_bytes(), b"");
            let v = CString::new("Show.S01E01").unwrap();
            nzbget_rs_nzb_set_text(p, 2, v.as_ptr(), -1);
            assert_eq!(
                CStr::from_ptr(nzbget_rs_nzb_text(p, 2)).to_bytes(),
                b"Show.S01E01"
            );
            nzbget_rs_nzb_set_text(p, 3, v.as_ptr(), 4);
            assert_eq!(CStr::from_ptr(nzbget_rs_nzb_text(p, 3)).to_bytes(), b"Show");
            nzbget_rs_nzb_set_text(p, 2, std::ptr::null(), -1);
            assert!(nzbget_rs_nzb_text(p, 2).is_null());
            (*p).id = 7;
            assert_eq!((*p).id, 7);
            nzbget_rs_nzb_scalars_free(p);
        }
    }

    #[test]
    fn params_through_the_typed_api() {
        let (p, q) = (nzbget_rs_nzb_scalars_new(), nzbget_rs_nzb_scalars_new());
        let (pn, qn) = (NzbRef::from_addr(p as usize), NzbRef::from_addr(q as usize));
        set_param(pn, b"Category", Some(b"tv"));
        assert_eq!(param(pn, b"category").as_deref(), Some(&b"tv"[..]));
        set_param(pn, b"CATEGORY", None);
        assert!(params(pn).is_empty());
        set_param(qn, b"a", Some(b"1"));
        set_param(pn, b"A", Some(b"0"));
        set_param(pn, b"b", Some(b"2"));
        copy_params(pn, qn);
        assert_eq!(params(pn), [(b"A".to_vec(), b"1".to_vec()), (b"b".to_vec(), b"2".to_vec())]);
        copy_params(pn, pn);
        assert_eq!(params(pn).len(), 2);
        assert_eq!(param(NzbRef::NONE, b"a"), None);
        assert!(!donor_blocked(pn, 7));
        block_donor(pn, 7);
        assert!(donor_blocked(pn, 7) && !donor_blocked(qn, 7));
        push_script_status(pn, b"script.py", ScriptStatus::from(2));
        assert_eq!(
            script_statuses(pn),
            [(b"script.py".to_vec(), ScriptStatus::from(2))]
        );
        unsafe {
            nzbget_rs_nzb_scalars_free(p);
            nzbget_rs_nzb_scalars_free(q);
        }
    }

    #[test]
    fn server_stats_and_script_total() {
        let p = nzbget_rs_nzb_scalars_new();
        let q = nzbget_rs_nzb_scalars_new();
        let (pn, qn) = (NzbRef::from_addr(p as usize), NzbRef::from_addr(q as usize));
        let stat = ServerStat::from([1, 5, 2]);
        apply_server_stat(pn, StatList::Total, StatOp::Add, stat);
        sync_current_server_stats(pn);
        assert_eq!(server_stats(pn, StatList::Current), [stat]);
        // merging adds both lists
        add_server_stats_from(qn, pn);
        add_server_stats_from(qn, pn);
        let twice = ServerStat::from([1, 10, 4]);
        assert_eq!(server_stats(qn, StatList::Total), [twice]);
        assert_eq!(server_stats(qn, StatList::Current), [twice]);
        apply_server_stat(qn, StatList::Total, StatOp::Subtract, stat);
        assert_eq!(server_stats(qn, StatList::Total), [stat]);
        apply_server_stat(
            qn,
            StatList::Total,
            StatOp::Set,
            ServerStat::from([1, 0, 0]),
        );
        assert_eq!(
            server_stats(qn, StatList::Total),
            [ServerStat::from([1, 0, 0])]
        );
        // copying onto itself doubles
        add_server_stats_from(pn, pn);
        assert_eq!(server_stats(pn, StatList::Total), [twice]);
        assert_eq!(script_total_status(pn), ScriptStatus::None);
        push_script_status(pn, b"a.py", ScriptStatus::Success);
        assert_eq!(script_total_status(pn), ScriptStatus::Success);
        assert_eq!(script_total_status(NzbRef::NONE), ScriptStatus::None);
        unsafe {
            nzbget_rs_nzb_scalars_free(p);
            nzbget_rs_nzb_scalars_free(q);
        }
    }

    #[test]
    fn completed_files_and_jobs() {
        use crate::queuesubsys_host::{CompletedFileSnap, StreamRepairJobDetailSnap};
        let p = coll_with_list();
        let q = coll_with_list();
        let (pn, qn) = (NzbRef::from_addr(p as usize), NzbRef::from_addr(q as usize));
        push_completed_file(
            qn,
            &CompletedFileSnap {
                id: 5,
                status: 3,
                crc: 77,
                filename: b"a.r00".to_vec(),
                hash16k: b"h".to_vec(),
                par_set_id: b"set".to_vec(),
                ..Default::default()
            },
        );
        // merging takes the other download's completed files
        merge_from(pn, qn);
        assert_eq!(completed_file_count(qn), 0);
        assert_eq!(completed_file_count(pn), 1);
        assert!(rename_completed_file(pn, b"a.r00", b"b.r00"));
        // failure becomes partial
        assert!(!update_completed_status(pn, 5, 2));
        let c = completed_file(pn, 0).unwrap();
        assert_eq!((c.id, c.status, c.par_file, c.crc), (5, 2, false, 77));
        assert_eq!(
            (c.filename.as_slice(), c.origname.as_slice()),
            (&b"b.r00"[..], &b"a.r00"[..])
        );
        assert!(update_completed_status(pn, 5, 1));
        assert!(!update_completed_status(pn, 5, 1));
        erase_completed_file(pn, 3);
        assert_eq!(completed_file_count(pn), 1);
        erase_completed_file(pn, 0);
        assert_eq!(completed_file_count(pn), 0);

        let range = |offset, size| ByteRange { offset, size };
        push_repair_job(
            pn,
            &StreamRepairJobDetailSnap {
                file_id: 9,
                failed_articles: 4,
                par_file: true,
                decoded_file_size: 100,
                failed_size: 30,
                missed_size: 2,
                steps_hash: 0xabc,
                filename: b"m.mkv".to_vec(),
                holes: vec![range(10, 5), range(40, 7)],
            },
        );
        assert_eq!(repair_job_count(pn), 1);
        let j = &repair_jobs(pn)[0];
        assert_eq!(
            (
                j.file_id,
                j.decoded_file_size,
                j.holes_total_size,
                j.failed_plus_missed_size
            ),
            (9, 100, 12, 32)
        );
        assert!(!mark_repair_job_live_attempted(pn, 0));
        assert!(mark_repair_job_live_attempted(pn, 0));
        update_repair_job_holes(pn, 0, 200, &[range(10, 5)]);
        let j = repair_job(pn, 0).unwrap();
        assert_eq!((j.file_id, j.failed_articles, j.par_file), (9, 4, true));
        assert_eq!(
            (
                j.decoded_file_size,
                j.failed_size,
                j.missed_size,
                j.steps_hash
            ),
            (200, 30, 2, 0xabc)
        );
        assert_eq!(j.holes.len(), 1);
        assert_eq!((j.holes[0].offset, j.holes[0].size), (10, 5));
        assert!(mark_repair_job_live_attempted(pn, 0));
        erase_repair_job(pn, 0, false);
        assert_eq!(repair_job_count(pn), 0);
        push_repair_job(pn, &StreamRepairJobDetailSnap::default());
        // replacing the files drops completed files and jobs
        push_completed_file(pn, &CompletedFileSnap::default());
        replace_files_from(pn, qn);
        assert_eq!((completed_file_count(pn), repair_job_count(pn)), (0, 0));
        free_coll_with_list(p);
        free_coll_with_list(q);
    }

    #[test]
    fn message_log() {
        let p = nzbget_rs_nzb_scalars_new();
        let q = nzbget_rs_nzb_scalars_new();
        let (pn, qn) = (NzbRef::from_addr(p as usize), NzbRef::from_addr(q as usize));
        unsafe {
            add_message(pn, 0, c"one".to_owned(), 100, 2);
            add_message(pn, 1, c"two".to_owned(), 100, 2);
            assert_eq!(add_message(pn, 2, c"three".to_owned(), 100, 2), 2);
            assert_eq!((*p).cachedMessageCount, 2);
            assert_eq!((*p).idMessageGen, 3);
            let got: Vec<(u32, i32, Vec<u8>)> = cached_messages(pn, MessageRange::Last(10))
                .into_iter()
                .map(|m| (m.id, m.kind, m.text.into_bytes()))
                .collect();
            assert_eq!(
                got,
                vec![(2, 1, b"two".to_vec()), (3, 2, b"three".to_vec())]
            );
            assert_eq!(cached_messages(pn, MessageRange::FromId(3)).len(), 1);
            assert_eq!(cached_messages(pn, MessageRange::FromId(1)).len(), 2);
            assert_eq!(cached_messages(pn, MessageRange::Last(1))[0].id, 3);
            // copies get fresh ids; a log never copies onto itself
            copy_cached_messages(qn, pn);
            assert_eq!((*q).cachedMessageCount, 2);
            assert_eq!((*q).idMessageGen, 2);
            copy_cached_messages(pn, pn);
            assert_eq!((*p).cachedMessageCount, 2);
            assert_eq!(add_message(NzbRef::NONE, 0, c"x".to_owned(), 0, 0), 0);
            nzbget_rs_nzb_scalars_free(p);
            nzbget_rs_nzb_scalars_free(q);
        }
    }

    #[test]
    fn post_state_and_lists() {
        use crate::queuesubsys_host::PostRef;
        let stage = PostField::Int(PostInt::StageProgress);
        unsafe {
            let p = nzbget_rs_post_new();
            let pr = PostRef::from_addr(p as usize);
            post_set(p, stage, 750);
            assert_eq!(post_get(p, stage), 750);
            assert_eq!(post_get(p, PostField::Thread), 0);
            post_set_progress_label(pr, b"Unpacking");
            assert_eq!(nzbget_rs_post_label(p), b"Unpacking");
            post_push_parred_file(pr, b"a.par2");
            assert_eq!(post_parred_files(pr), vec![b"a.par2".to_vec()]);
            // without a collection the job keeps its own extracted list
            assert!(post_extracted_archives(pr).is_empty());
            (*p).lists
                .lock()
                .unwrap()
                .extracted
                .push(c"x.rar".to_owned());
            assert_eq!(post_extracted_archives(pr), vec![b"x.rar".to_vec()]);
            post_clear_extracted_archives(pr);
            assert!(post_extracted_archives(pr).is_empty());
            nzbget_rs_post_free(p);
        }
    }

    #[test]
    fn post_fields_follow_the_collection() {
        use crate::queuesubsys_host::PostRef;
        let f = PostField::Int;
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            let p = nzbget_rs_post_new();
            let pr = PostRef::from_addr(p as usize);
            // without a collection the post job keeps its own values
            post_set(p, f(PostInt::ParRepaired), 1);
            assert_eq!(post_get(p, f(PostInt::ParRepaired)), 1);
            assert!(!(*n).parRepaired);
            set_post_nzb(p, n);
            assert_eq!(post_nzb(p), n);
            // reads come from the collection, writes go to both
            assert_eq!(post_get(p, f(PostInt::ParRepaired)), 0);
            post_set(p, f(PostInt::UnpackTried), 1);
            post_set(p, f(PostInt::LastUnpackStatus), 2);
            assert!((*n).unpackTried);
            assert_eq!((*n).lastUnpackStatus, 2);
            (*n).lastUnpackStatus = 3;
            assert_eq!(post_get(p, f(PostInt::LastUnpackStatus)), 3);
            // stage progress stays in 0..=1000
            post_set(p, f(PostInt::StageProgress), 5000);
            assert_eq!(post_get(p, f(PostInt::StageProgress)), 1000);
            post_set(p, f(PostInt::StageProgress), -4);
            assert_eq!(post_get(p, f(PostInt::StageProgress)), 0);
            // extracted archives are the collection's list
            push_extracted_archive(NzbRef::from_addr(n as usize), b"a.rar");
            assert_eq!(post_extracted_archives(pr), vec![b"a.rar".to_vec()]);
            post_clear_extracted_archives(pr);
            assert!(data(n).files().extracted.is_empty());
            nzbget_rs_post_free(p);
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    #[test]
    fn coll_set_enters_and_leaves_post_process() {
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            // stage and force flags need a post job
            set_int(NzbRef::from_addr(n as usize), NzbInt::PostStage, 4);
            assert_eq!(slot(n, Slot::Post), 0);
            act(NzbRef::from_addr(n as usize), NzbAction::EnterPostProcess);
            let p = slot(n, Slot::Post) as *const Post;
            assert!(!p.is_null());
            assert_eq!(post_get(p, PostField::Int(PostInt::Deleted)), 0);
            assert_eq!(post_nzb(p), n);
            set_int(NzbRef::from_addr(n as usize), NzbInt::PostStage, 7);
            set_int(NzbRef::from_addr(n as usize), NzbInt::PostForceFlags, 2);
            assert_eq!(post_get(p, PostField::Int(PostInt::Stage)), 7);
            assert_eq!(post_get(p, PostField::Int(PostInt::ForceParFull)), 0);
            assert_eq!(post_get(p, PostField::Int(PostInt::ForceRepair)), 1);
            // a second enter replaces the post job
            act(NzbRef::from_addr(n as usize), NzbAction::EnterPostProcess);
            let p2 = slot(n, Slot::Post) as *const Post;
            assert_eq!(post_get(p2, PostField::Int(PostInt::Stage)), 0);
            post_set(p2, PostField::Int(PostInt::UnpackTried), 1);
            post_set(p2, PostField::Int(PostInt::LastUnpackStatus), 2);
            push_extracted_archive(NzbRef::from_addr(n as usize), b"x.rar");
            act(NzbRef::from_addr(n as usize), NzbAction::LeavePostProcess);
            assert_eq!(slot(n, Slot::Post), 0);
            assert!(!(*n).unpackTried);
            assert_eq!((*n).lastUnpackStatus, 0);
            assert!(data(n).files().extracted.is_empty());
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    #[test]
    fn post_snapshot_and_typed_setters() {
        use crate::queuesubsys_host::PostRef;
        unsafe {
            // a missing post job gives the default snapshot
            let none = post_snapshot(PostRef::NONE);
            assert!(none.ptr.is_none() && !none.working && none.stage_progress == 0);
            let p = nzbget_rs_post_new();
            let pr = PostRef::from_addr(p as usize);
            // flags are stored as 0/1
            post_set_int(pr, PostInt::Working, 5);
            post_set_int(pr, PostInt::ForceRepair, 1);
            post_set_int(pr, PostInt::Stage, 7);
            post_set_int(pr, PostInt::LastUnpackStatus, 3);
            assert_eq!(post_get(p, PostField::Int(PostInt::Working)), 1);
            post_set_start_time(pr, 1234);
            post_set_int(pr, PostInt::StageProgress, 500);
            post_set_progress_label(pr, b"Repairing");
            post_push_parred_file(pr, b"a.par2");
            post_push_parred_file(pr, b"");
            assert_eq!(post_parred_files(pr), vec![b"a.par2".to_vec(), Vec::new()]);
            assert!(post_extracted_archives(pr).is_empty());
            (*p).lists
                .lock()
                .unwrap()
                .extracted
                .push(c"b.rar".to_owned());
            assert_eq!(post_extracted_archives(pr), vec![b"b.rar".to_vec()]);
            let snap = post_snapshot(pr);
            assert_eq!(snap.ptr, pr);
            assert_eq!(i32::from(snap.stage), 7);
            assert!(snap.working && snap.force_repair && !snap.deleted);
            assert!(!snap.has_post_thread && !snap.post_thread_running);
            assert_eq!(i32::from(snap.last_unpack_status), 3);
            assert_eq!(snap.stage_progress, 500);
            assert_eq!((snap.start_time, snap.stage_time), (1234, 0));
            assert!(snap.nzb_ptr.is_none());
            assert_eq!(snap.progress_label, b"Repairing");
            // a new stage clears progress and label
            post_begin_stage(pr, 99);
            let snap = post_snapshot(pr);
            assert_eq!(
                (snap.stage_progress, snap.file_progress, snap.stage_time),
                (0, 0, 99)
            );
            assert!(snap.progress_label.is_empty());
            assert!(nzbget_rs_post_label(p).is_empty());
            post_set_stage_time(pr, 77);
            assert_eq!(post_get(p, PostField::StageTime), 77);
            post_clear_extracted_archives(pr);
            assert!(post_extracted_archives(pr).is_empty());
            nzbget_rs_post_free(p);
            // a missing handle is a no-op
            post_set_int(PostRef::NONE, PostInt::Working, 1);
        }
    }

    #[test]
    fn host_post_job_op_without_jobs() {
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            let p = nzbget_rs_post_new();
            let i64s = [0i64];
            for a1 in [0, 1, 2, 11] {
                assert_eq!(host_post_job_op(a1, 1, p as usize, 0, i64s.as_ptr()), 0);
            }
            for a1 in [3, 9, 10] {
                assert_eq!(host_post_job_op(a1, 0, n as usize, 0, i64s.as_ptr()), 0);
            }
            for a1 in 4..=7 {
                assert_eq!(host_post_job_op(a1, 0, 0, n as usize, i64s.as_ptr()), 0);
            }
            for a1 in [0, 1, 2, 3, 8, 9, 10, 11, 12] {
                assert_eq!(host_post_job_op(a1, 0, 0, 0, i64s.as_ptr()), 0);
            }
            // direct unpack queries need a job
            assert_eq!(host_post_job_op(3, 1, n as usize, 0, i64s.as_ptr()), 0);
            // the script controller is skipped when the collection skips scripts
            set_post_nzb(p, n);
            (*n).skipScriptProcessing = true;
            assert_eq!(host_post_job_op(12, 9, p as usize, 0, i64s.as_ptr()), 0);
            assert_eq!(post_get(p, PostField::Thread), 0);
            nzbget_rs_post_free(p);
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    unsafe extern "C" fn drop_test_file(p: usize) {
        unsafe { crate::qfile::nzbget_rs_file_free(p as *mut crate::qfile::File) };
    }

    /// A collection with a file list, as the host's constructor makes it.
    fn coll_with_list() -> *mut NzbScalars {
        let n = nzbget_rs_nzb_scalars_new();
        let l = crate::qlist::nzbget_rs_ptrlist_new(drop_test_file);
        swap_slot(n, Slot::FileList, l as usize);
        n
    }

    fn free_coll_with_list(n: *mut NzbScalars) {
        unsafe {
            let l = swap_slot(n, Slot::FileList, 0);
            crate::qlist::nzbget_rs_ptrlist_free(l as *mut crate::qlist::PtrList);
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    fn add_test_file(n: *mut NzbScalars, fields: &[(c_int, i64)]) -> *const crate::qfile::File {
        let f = crate::qfile::nzbget_rs_file_new(-1);
        unsafe {
            for &(id, v) in fields {
                crate::qfile::nzbget_rs_file_set(f, id, v);
            }
            crate::qfile::nzbget_rs_file_set(f, crate::qfile::field::NZB, n as i64);
        }
        ptr_list_op(file_list(n), 3, 0, f as usize, std::ptr::null(), 0);
        f
    }

    fn text_of(n: *const NzbScalars, id: c_int) -> Option<Vec<u8>> {
        let t = unsafe { nzbget_rs_nzb_text(n, id) };
        (!t.is_null()).then(|| unsafe { CStr::from_ptr(t) }.to_bytes().to_vec())
    }

    #[test]
    fn coll_set_filename_and_url_name_the_collection() {
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            (*n).id = 5;
            set_str(
                NzbRef::from_addr(n as usize),
                NzbStr::Filename,
                Some(b"/tmp/My.Show.nzb"),
            );
            assert_eq!(text_of(n, 1).unwrap(), b"/tmp/My.Show.nzb");
            assert_eq!(text_of(n, 2).unwrap(), b"My.Show");
            // a second file name keeps the name taken from the first
            set_str(
                NzbRef::from_addr(n as usize),
                NzbStr::Filename,
                Some(b"Other.nzb"),
            );
            assert_eq!(text_of(n, 2).unwrap(), b"My.Show");
            // a URL leaves an existing name alone
            set_str(
                NzbRef::from_addr(n as usize),
                NzbStr::Url,
                Some(b"http://host/x.nzb"),
            );
            assert_eq!(text_of(n, 0).unwrap(), b"http://host/x.nzb");
            assert_eq!(text_of(n, 2).unwrap(), b"My.Show");
            nzbget_rs_nzb_scalars_free(n);

            let u = nzbget_rs_nzb_scalars_new();
            set_str(
                NzbRef::from_addr(u as usize),
                NzbStr::Url,
                Some(b"http://host.example/get/123"),
            );
            assert_eq!(text_of(u, 2).unwrap(), b"host.example/get/123");
            // a collection named after no file yet takes the file's name
            set_str(
                NzbRef::from_addr(u as usize),
                NzbStr::Filename,
                Some(b"Real.Name.nzb"),
            );
            assert_eq!(text_of(u, 2).unwrap(), b"Real.Name");
            // an empty file name never renames
            set_str(NzbRef::from_addr(u as usize), NzbStr::Filename, Some(b""));
            assert_eq!(text_of(u, 2).unwrap(), b"Real.Name");
            nzbget_rs_nzb_scalars_free(u);

            // the nice URL name comes from the URL and the NZB file name only
            assert_eq!(
                crate::downloadinfo::make_nice_url_name(
                    Some(c"https://idx.example/api"),
                    b"Some.Name.nzb"
                ),
                b"Some.Name @ idx.example"
            );
        }
    }

    #[test]
    fn coll_dir_names_end_with_the_collection_name() {
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            (*n).id = 9;
            nzbget_rs_nzb_set_text(n, 2, c"Some.Name".as_ptr(), -1);
            let final_dir = build_final_dir_name(NzbRef::from_addr(n as usize));
            assert!(final_dir.ends_with(b"Some.Name"), "{final_dir:?}");
            act(NzbRef::from_addr(n as usize), NzbAction::BuildDestDir);
            let dest = text_of(n, 3).unwrap();
            assert!(dest.windows(9).any(|w| w == b"Some.Name"), "{dest:?}");
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    #[test]
    fn coll_set_active_downloads_accounts_download_time() {
        let mut s = NzbScalars::default();
        set_active_downloads(&mut s, 2, 1000);
        assert_eq!(s.activeDownloads, 2);
        assert_eq!(s.downloadStartTime, 1000);
        set_active_downloads(&mut s, 0, 1030);
        assert_eq!(s.activeDownloads, 0);
        assert_eq!(s.downloadSec, 30);
        assert!(s.changed);
    }

    #[test]
    fn coll_file_stats_and_times_follow_the_file_list() {
        use crate::qfile::field;
        unsafe {
            let n = coll_with_list();
            let f1 = add_test_file(
                n,
                &[
                    (field::TIME, 300),
                    (field::SIZE, 100),
                    (field::REMAINING_SIZE, 60),
                    (field::SUCCESS_SIZE, 30),
                    (field::FAILED_SIZE, 10),
                    (field::SUCCESS_ARTICLES, 3),
                    (field::FAILED_ARTICLES, 1),
                    (field::TOTAL_ARTICLES, 10),
                ],
            );
            add_test_file(
                n,
                &[
                    (field::TIME, 100),
                    (field::SIZE, 50),
                    (field::REMAINING_SIZE, 50),
                    (field::PAR_FILE, 1),
                    (field::PAUSED, 1),
                ],
            );
            act(NzbRef::from_addr(n as usize), NzbAction::UpdateMinMaxTime);
            assert_eq!(((*n).minTime, (*n).maxTime), (100, 300));

            (*n).successArticles = 5;
            (*n).successSize = 500;
            act(NzbRef::from_addr(n as usize), NzbAction::UpdateCurrentStats);
            assert_eq!((*n).remainingSize, 110);
            assert_eq!((*n).pausedFileCount, 1);
            assert_eq!((*n).pausedSize, 50);
            assert_eq!((*n).remainingParCount, 1);
            assert_eq!((*n).currentSuccessArticles, 8);
            assert_eq!((*n).currentFailedArticles, 1);
            assert_eq!((*n).currentSuccessSize, 530);
            assert_eq!((*n).currentFailedSize, 10);

            let before = (*n).successSize;
            add_file_totals(
                NzbRef::from_addr(n as usize),
                crate::queuesubsys_host::FileRef::from_addr(f1 as usize),
                FileTotals::Completed,
            );
            assert_eq!((*n).successSize, before + 30);
            assert_eq!((*n).failedArticles, 1);

            (*n).fileCount = 2;
            (*n).size = 150;
            add_file_totals(
                NzbRef::from_addr(n as usize),
                crate::queuesubsys_host::FileRef::from_addr(f1 as usize),
                FileTotals::Deleted,
            );
            assert_eq!((*n).fileCount, 1);
            assert_eq!((*n).size, 50);
            assert_eq!((*n).totalArticles, -10);

            act(NzbRef::from_addr(n as usize), NzbAction::ClearFileList);
            assert_eq!(ptr_list_op(file_list(n), 0, 0, 0, std::ptr::null(), 0), 0);
            free_coll_with_list(n);
        }
    }

    #[test]
    fn coll_active_output_needs_an_open_downloading_file() {
        use crate::qfile::field;
        unsafe {
            let n = coll_with_list();
            let op = |n: *const NzbScalars| has_busy_output_file(NzbRef::from_addr(n as usize));
            assert!(!op(n));
            let f = add_test_file(
                n,
                &[(field::OUTPUT_INITIALIZED, 1), (field::ACTIVE_DOWNLOADS, 1)],
            );
            // no output file name yet
            assert!(!op(n));
            crate::qfile::nzbget_rs_file_set_text(
                f,
                crate::qfile::text::OUTPUT_FILENAME,
                c"out.tmp".as_ptr(),
            );
            assert!(op(n));
            crate::qfile::nzbget_rs_file_set(f, field::ACTIVE_DOWNLOADS, 0);
            assert!(!op(n));
            free_coll_with_list(n);
        }
    }

    #[test]
    fn nzb_snapshot_reports_fields_and_computed_values() {
        use crate::qfile::field;
        assert_eq!(nzb_snapshot(NzbRef::NONE, TextStatus::IgnoreScript).id, 0);
        unsafe {
            let n = coll_with_list();
            let r = NzbRef::from_addr(n as usize);
            (*n).id = 77;
            (*n).kind = 0;
            (*n).priority = 900;
            (*n).extraPriority = 1;
            (*n).desiredServerId = 3;
            (*n).size = 1000;
            (*n).currentFailedSize = 100;
            (*n).fullContentHash = 0xdead_beef;
            (*n).filteredContentHash = 0xffff_0001;
            nzbget_rs_nzb_set_text(n, 2, c"Name".as_ptr(), -1);
            nzbget_rs_nzb_set_text(n, 0, c"http://u".as_ptr(), -1);
            add_test_file(n, &[(field::PAR_FILE, 1), (field::PAUSED, 1)]);
            let s = nzb_snapshot(r, TextStatus::IgnoreScript);
            assert_eq!(s.ptr, r);
            assert_eq!(s.id, 77);
            assert!(s.force_priority && s.has_extra_priority && s.has_desired_server);
            assert_eq!(s.desired_server_id, 3);
            assert!(!s.has_post_info && s.post_info_ptr.is_none());
            assert_eq!(s.health, 900);
            // a paused par file blocks completion only when paused pars count
            assert!(s.is_download_completed && !s.is_download_completed_strict);
            assert_eq!(s.dupe_failover_checked, -1);
            assert_eq!(s.size, 1000);
            assert_eq!(
                (s.full_content_hash, s.filtered_content_hash),
                (0xdead_beef, 0xffff_0001)
            );
            assert_eq!(s.name, b"Name");
            assert_eq!(s.url, b"http://u");
            assert!(s.filename.is_empty());
            assert!(!s.text_status.is_empty() && !s.text_status.contains(&0));
            assert!(nzb_snapshot(r, TextStatus::Off).text_status.is_empty());
            // with a post job
            act(r, NzbAction::EnterPostProcess);
            set_int(r, NzbInt::PostStage, 7);
            set_int(r, NzbInt::PostForceFlags, 2);
            let s = nzb_snapshot(r, TextStatus::Off);
            assert!(s.has_post_info && s.post_info_ptr.is_some());
            assert_eq!(i32::from(s.post_stage), 7);
            assert!(!s.force_par_full && s.force_repair);
            // active downloads: never completed
            (*n).activeDownloads = 1;
            let s = nzb_snapshot(r, TextStatus::Off);
            assert!(!s.is_download_completed && !s.is_download_completed_strict);
            act(r, NzbAction::LeavePostProcess);
            free_coll_with_list(n);
        }
    }
}
