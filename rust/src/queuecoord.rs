//! Rust implementation of `QueueCoordinator` (`daemon/queue/QueueCoordinator.cpp`).

use crate::queuesubsys_host::NzbRef;
use crate::queuesubsys_host::FileRef;
use crate::status::{DeleteStatus, DirectRenameStatus, DirectUnpackStatus, DupeMode, PostStage};
use std::ffi::CStr;

#[inline]
pub fn strcasecmp_bytes(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[inline]
pub fn find_subslice_ci(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|w| strcasecmp_bytes(w, needle))
}

// ============================================================================
// Status Constants Shared with HistoryCoordinator
// ============================================================================

pub const ARTICLE_UNDEFINED: i32 = 0;
pub const ARTICLE_RUNNING: i32 = 1;

/// Downloads whose first volume's headers were written, waiting to start direct unpack, with
/// when to try (`liveorder::now_ms`; see `drain_live_heads`).
static PENDING_HEADS: std::sync::Mutex<Vec<(i32, u64)>> = std::sync::Mutex::new(Vec::new());
/// How long a download that could not start direct unpack from its headers waits to try again.
const LIVE_HEAD_RETRY_MS: u64 = 500;
/// How often running articles are checked for ones far slower than the rest.
const STRAGGLER_CHECK_MS: u64 = 250;
static NEXT_STRAGGLER_CHECK_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A download's live extraction order (`liveorder`) and its lowest unfinished volume.
struct LiveOrder {
    ranks: std::sync::Arc<crate::liveorder::Ranks>,
    lowest: Option<usize>,
}

impl LiveOrder {
    /// The rank of the file named `filename` when it is a volume within the window.
    fn window_rank(&self, filename: &[u8]) -> Option<usize> {
        let rank = self.ranks.of(filename)?;
        self.lowest
            .filter(|&l| crate::liveorder::in_window(rank, l))
            .map(|_| rank)
    }
}

/// The volumes, in extraction order, of the set whose first volume is named `first`, from the
/// names of download `nzb_ptr`'s files: what direct unpack registers once it starts
/// (`liveorder::set_order`).
fn live_set_order(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: NzbRef,
    first: Option<&[u8]>,
) -> Option<Vec<Vec<u8>>> {
    let mut names: Vec<Vec<u8>> = (0..host.nzb_file_len(nzb_ptr))
        .map(|i| host.file_snap(host.nzb_file_at(nzb_ptr, i)))
        .filter(|f| !f.deleted)
        .map(|f| crate::cstr::until_nul(&f.filename).to_vec())
        .collect();
    names.extend((0..host.nzb_completed_files_len(nzb_ptr)).map(|i| {
        crate::cstr::until_nul(&host.nzb_completed_file_at(nzb_ptr, i).filename).to_vec()
    }));
    // Without a first volume whose headers are written: the first one the names show.
    let first = match first {
        Some(f) => f.to_vec(),
        None => names
            .iter()
            .find(|n| crate::unpack::direct_unpack_is_main_archive(n))?
            .clone(),
    };
    if crate::livefeed::is_live_first(&first) {
        return crate::livefeed::live_set(&first, &names).map(|set| set.volumes);
    }
    let rars = names
        .iter()
        .filter(|n| crate::filetypes::is_rar_file(n))
        .count() as u64;
    (rars > 1).then(|| crate::postproc::rar_volume_names(&first, rars))
}

/// Whether direct unpack may extract download `nzb_ptr` live at all: it is on, the download
/// writes its files to disk, and unpacking is not turned off for it. Only such downloads get an
/// extraction order (and their slow articles asked again).
fn live_extraction_allowed(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: NzbRef,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    let opts = host.opts();
    opts.direct_unpack
        && !opts.raw_article
        && !opts.skip_write
        && !nzb.skip_disk_write
        && host
            .nzb_get_param(nzb_ptr, b"*Unpack:")
            .as_ref()
            .is_none_or(|v| v.eq_ignore_ascii_case(b"yes"))
}

/// The name of the first volume download `nzb_id` has written the headers of, if any.
fn head_name(nzb_id: i32) -> Option<Vec<u8>> {
    let heads = crate::livewrite::heads(nzb_id);
    let path = heads.first()?;
    Some(match path
        .iter()
        .rposition(|&b| b == b'/' || b == crate::paths::PATH_SEPARATOR)
    {
        Some(i) => path[i + 1..].to_vec(),
        None => path.clone(),
    })
}

/// Downloads whose file names were looked at for a set to take in extraction order.
static ORDER_BY_NAME_SEEN: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());
static NEXT_ORDER_BY_NAME_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The live extraction order of download `nzb_id` (`nzb_ptr`), from `cache` or looked up (once
/// per download): the files still queued give the lowest unfinished volume.
fn cached_live_order<'c>(
    cache: &'c mut Vec<(i32, Option<LiveOrder>)>,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: NzbRef,
    nzb_id: i32,
) -> Option<&'c LiveOrder> {
    if !crate::liveorder::any() {
        return None;
    }
    let at = match cache.iter().position(|(id, _)| *id == nzb_id) {
        Some(at) => at,
        None => {
            let order = crate::liveorder::ranks(nzb_id).map(|ranks| {
                let lowest = (0..host.nzb_file_len(nzb_ptr))
                    .filter_map(|j| {
                        let f = host.file_snap(host.nzb_file_at(nzb_ptr, j));
                        // (a paused volume waits: the window goes on past it)
                        (!f.deleted && !f.paused).then(|| ranks.of(&f.filename)).flatten()
                    })
                    .min();
                LiveOrder { ranks, lowest }
            });
            cache.push((nzb_id, order));
            cache.len() - 1
        }
    };
    cache[at].1.as_ref()
}
pub const ARTICLE_FINISHED: i32 = 2;
pub const ARTICLE_FAILED: i32 = 3;

pub const PARTIAL_STATE_NONE: i32 = 0;
pub const PARTIAL_STATE_PARTIAL: i32 = 1;
pub const PARTIAL_STATE_COMPLETED: i32 = 2;

pub const COMPLETED_FILE_NONE: i32 = 0;
pub const COMPLETED_FILE_SUCCESS: i32 = 1;
pub const COMPLETED_FILE_PARTIAL: i32 = 2;
pub const COMPLETED_FILE_FAILURE: i32 = 3;

/// `QueueCoordinator::DeleteFileInfo` (`QueueCoordinator.cpp`):
/// Classifies `CompletedFile::EStatus` from article counts.
pub fn classify_completed_file_status(
    total_articles: i32,
    success_articles: i32,
    missed_articles: i32,
    failed_articles: i32,
) -> i32 {
    if total_articles == success_articles {
        COMPLETED_FILE_SUCCESS
    } else if total_articles == missed_articles + failed_articles {
        COMPLETED_FILE_FAILURE
    } else if success_articles > 0 || failed_articles > 0 {
        COMPLETED_FILE_PARTIAL
    } else {
        COMPLETED_FILE_NONE
    }
}

/// Base filename after last `/` or `\`.
pub fn base_file_name(path: &[u8]) -> &[u8] {
    if let Some(pos) = path.iter().rposition(|&b| b == b'/' || b == b'\\') {
        &path[pos + 1..]
    } else {
        path
    }
}

/// `QueueCoordinator::DeleteFileInfo` (`QueueCoordinator.cpp`):
/// Resolves the filename recorded in `CompletedFile`, preserving relative subfolder
/// paths under `dest_dir` and falling back to `file_filename` when `cfFailure` never
/// created `output_filename` on disk.
pub fn resolve_completed_filename(
    completed: bool,
    file_status: i32,
    output_filename: &[u8],
    file_filename: &[u8],
    dest_dir: &[u8],
    path_sep: u8,
    output_file_exists: bool,
) -> Vec<u8> {
    let mut filename: Vec<u8> = if completed && !output_filename.is_empty() {
        base_file_name(output_filename).to_vec()
    } else {
        file_filename.to_vec()
    };

    let mut dest_prefix = Vec::with_capacity(dest_dir.len() + 1);
    dest_prefix.extend_from_slice(dest_dir);
    dest_prefix.push(path_sep);

    if completed
        && !output_filename.is_empty()
        && output_filename.len() > dest_prefix.len()
        && output_filename.starts_with(&dest_prefix)
        && output_filename[dest_prefix.len()..].contains(&path_sep)
    {
        filename = output_filename[dest_prefix.len()..].to_vec();
    }

    if completed
        && file_status == COMPLETED_FILE_FAILURE
        && !file_filename.is_empty()
        && !output_file_exists
    {
        filename = file_filename.to_vec();
    }

    filename
}

// ============================================================================
// QueueCoordinator (daemon/queue/QueueCoordinator.cpp)
// ============================================================================

/// `QueueCoordinator::AdjustDownloadsLimit` (`QueueCoordinator.cpp`):
/// Base 2 threads + maxConnections for each active level-0 or level-1 server.
pub fn calc_downloads_limit(norm_levels: &[i32], actives: &[u8], max_conns: &[i32]) -> i32 {
    let mut limit = 2i32;
    let n = norm_levels.len().min(actives.len()).min(max_conns.len());
    for i in 0..n {
        if (norm_levels[i] == 0 || norm_levels[i] == 1) && actives[i] != 0 {
            limit += max_conns[i];
        }
    }
    limit
}

/// `QueueCoordinator::IsPropagationWait` (`QueueCoordinator.cpp`).
#[inline]
pub fn is_propagation_wait(post_time: i64, propagation_delay: i32, cur_date: i64) -> bool {
    propagation_delay > 0 && (post_time as i32).wrapping_add(propagation_delay) >= (cur_date as i32)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RsCandidateFile {
    pub nzb_priority: i32,
    pub extra_priority: i32,
    pub paused: i32,
    pub deleted: i32,
    pub already_checked: i32,
    pub post_time: i64,
}

/// `QueueCoordinator::GetNextArticle` (`QueueCoordinator.cpp`) inner candidate selection:
/// Selects the best runnable file index (`out_best_idx`, or `-1`) and the earliest
/// propagation-held file index (`out_prop_wait_idx`, or `-1`).
pub fn select_next_file(
    files: &[RsCandidateFile],
    propagation_delay: i32,
    cur_date: i64,
) -> (i32, i32) {
    let mut best_idx: Option<usize> = None;
    let mut prop_wait_idx: Option<usize> = None;

    for (idx, f) in files.iter().enumerate() {
        if f.already_checked != 0 || f.paused != 0 || f.deleted != 0 {
            continue;
        }
        let prop_wait = is_propagation_wait(f.post_time, propagation_delay, cur_date);
        if !prop_wait {
            let higher = match best_idx {
                None => true,
                Some(prev_i) => {
                    let prev = &files[prev_i];
                    let cur_ep = f.extra_priority != 0;
                    let prev_ep = prev.extra_priority != 0;
                    (cur_ep == prev_ep && f.nzb_priority > prev.nzb_priority)
                        || (cur_ep && !prev_ep)
                }
            };
            if higher {
                best_idx = Some(idx);
            }
        } else {
            let earlier = match prop_wait_idx {
                None => true,
                Some(prev_i) => f.post_time < files[prev_i].post_time,
            };
            if earlier {
                prop_wait_idx = Some(idx);
            }
        }
    }

    (
        best_idx.map(|i| i as i32).unwrap_or(-1),
        prop_wait_idx.map(|i| i as i32).unwrap_or(-1),
    )
}

/// `QueueCoordinator::CertainlyParless` `dataName` check (`QueueCoordinator.cpp`).
pub fn is_data_filename(filename: &[u8]) -> bool {
    let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') else {
        return false;
    };
    let ext = &filename[dot_pos..];
    if ext.len() <= 1 {
        return false;
    }
    const EXTENSIONS: &[&[u8]] = &[
        b".mkv", b".mp4", b".avi", b".m4v", b".ts", b".m2ts", b".mov", b".wmv", b".mpg", b".mpeg",
        b".iso", b".rar", b".7z", b".zip", b".nfo", b".sfv", b".srr", b".srt", b".sub", b".idx",
        b".jpg", b".png", b".txt", b".flac", b".mp3", b".m4a",
    ];
    for &known in EXTENSIONS {
        if strcasecmp_bytes(ext, known) {
            return true;
        }
    }
    let digits = if ext[1] == b'r' || ext[1] == b'R' {
        &ext[2..]
    } else {
        &ext[1..]
    };
    digits.len() >= 2 && digits.len() <= 3 && digits.iter().all(|b| b.is_ascii_digit())
}

/// `QueueCoordinator::HasSizeTwin` `sizeNear` check (`QueueCoordinator.cpp`):
/// `llabs(size1 - size2) * 1000 <= size1 * 3`
#[inline]
pub fn size_near(size1: i64, size2: i64) -> bool {
    (size1 - size2).wrapping_abs().saturating_mul(1000) <= size1.saturating_mul(3)
}

pub const DUPE_HOPELESS_TRIED_PERCENT: i64 = 10;
pub const DUPE_HOPELESS_ALIVE_PERCENT: i64 = 10;
pub const DEAD_DOWNLOAD_FAILURES: i32 = 64;
pub const DEAD_RUN_FAILURES: i32 = 40;
pub const PROJECTED_FAILURE_SAMPLE: i32 = 200;
pub const SLOW_PROBE_SEC: i32 = 10;
pub const PARK_BELOW_CRITICAL_SAMPLE: i32 = 1000;

/// A download is failing in bulk: at least `DUPE_MASS_FAILURE_MIN` articles and
/// `DUPE_MASS_FAILURE_PERCENT` percent of the collection could not be fetched. Below that,
/// option DupeBorrowFirst repairs from duplicates instead of failing over.
pub const DUPE_MASS_FAILURE_MIN: i32 = 20;
pub const DUPE_MASS_FAILURE_PERCENT: i64 = 2;

pub fn dupe_mass_failure(total_articles: i32, failed_articles: i32) -> bool {
    failed_articles >= DUPE_MASS_FAILURE_MIN
        && (failed_articles as i64) * 100 >= (total_articles as i64) * DUPE_MASS_FAILURE_PERCENT
}

/// `QueueCoordinator::DownloadHopeless` (`QueueCoordinator.cpp`).
pub fn download_hopeless(total: i32, current_success: i32, current_failed: i32) -> bool {
    let tried = current_success + current_failed;
    total > 0
        && (tried as i64) * 100 >= (total as i64) * DUPE_HOPELESS_TRIED_PERCENT
        && (current_success as i64) * 100 < (tried as i64) * DUPE_HOPELESS_ALIVE_PERCENT
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RsDeadDownloadEval {
    pub own: i32,
    pub tried: i32,
    pub dead_rate: i32,
    pub dead_run: i32,
    pub dead: i32,
    pub projected: i32,
    pub doomed: i32,
    pub hopeless: i32,
    pub far_below: i32,
    pub below: i32,
}

/// `QueueCoordinator::CheckDeadDownload` metrics (`QueueCoordinator.cpp`).
#[allow(clippy::too_many_arguments)]
/// Of `articles` covering `size` bytes, about how many are not par2 files' (`par_size` of
/// the bytes): par2 articles are assumed as large as the others.
fn data_articles(articles: i32, size: i64, par_size: i64) -> i32 {
    if size <= 0 || par_size <= 0 {
        return articles;
    }
    let data = (size - par_size).clamp(0, size);
    (i128::from(articles.max(0)) * i128::from(data) / i128::from(size)) as i32
}

pub fn eval_dead_download(
    current_success: i32,
    dupe_recovered: i32,
    current_failed: i32,
    file_count: i32,
    dupe_failed_run: i32,
    par_cannot_cover_exact: bool,
    critical: i32,
    par_edge_margin: i32,
    files_tried: i32,
    probing_sec: i32,
) -> RsDeadDownloadEval {
    let own = (current_success - dupe_recovered.max(0)).max(0);
    let failed = current_failed;
    let tried = own + failed;

    let dead_rate =
        failed >= DEAD_DOWNLOAD_FAILURES + file_count && (own as i64) * 100 < (failed as i64);
    let dead_run = dupe_failed_run >= DEAD_RUN_FAILURES && par_cannot_cover_exact;
    let dead = dead_rate || dead_run;

    let projected = if tried > 0 {
        ((own as i64) * 1000 / (tried as i64)) as i32
    } else {
        1000
    };
    let doomed = !dead
        && tried >= PROJECTED_FAILURE_SAMPLE
        && files_tried >= 3
        && projected < critical + par_edge_margin;

    let hopeless = own == 0 && failed >= PROJECTED_FAILURE_SAMPLE && probing_sec >= SLOW_PROBE_SEC;
    let far_below = projected * 2 < critical;
    let below = projected < critical && tried >= PARK_BELOW_CRITICAL_SAMPLE;

    RsDeadDownloadEval {
        own,
        tried,
        dead_rate: i32::from(dead_rate),
        dead_run: i32::from(dead_run),
        dead: i32::from(dead),
        projected,
        doomed: i32::from(doomed),
        hopeless: i32::from(hopeless),
        far_below: i32::from(far_below),
        below: i32::from(below),
    }
}

/// True when a download that lost a stretch of articles should keep going instead of failing
/// over: its backup is a twin or an alt in a borrowable packing (the same video, stored), live
/// borrowing and direct unpack are on (`live_borrowing`; only direct unpack reads a rebuilt dead
/// volume), the download's own packing is not unborrowable, and at least half of what
/// it tried arrived. The lost parts are then borrowed from the backup (whole dead volumes
/// included, see `deadvolume`), and the extraction already running is kept; a failover would
/// delete it and download and extract the backup from its first byte. A download whose
/// articles are mostly gone (`dead_rate`) still fails over, unless direct unpack of it runs
/// (`extracting`): the backup then feeds the rest of that extraction, however much is gone.
///
/// A backup whose kind is not known yet (an alt nested in another container, say: its first
/// file is an archive, not the video) lends too once a borrow from it is underway, or while
/// direct unpack of the download runs (`extracting`) if it was labelled the same video in
/// another container, or a proven twin (`labelled_same_video`, see `backup_may_lend_while_extracting`;
/// a `container` label counts too, the borrow checks the video): the
/// first dead volume fails many articles in a row before it ends, which is before any borrow
/// can start, and failing over then would throw the running extraction away. An unlabelled
/// backup still fails over.
///
/// A label is no proof of the same video ("another container" says nothing of what is
/// inside), so a backup whose video the live dead-volume borrow checked and found to differ
/// (`backup_mismatched`, see `postproc::dead_volume_donor_cannot_lend`) lends nothing, whatever
/// its kind: the download fails over then, instead of downloading to its end first. It is not
/// called mismatched while another donor lends the dead volumes.
#[allow(clippy::too_many_arguments)]
pub fn alt_covers_lost_articles(
    dead_rate: bool,
    live_borrowing: bool,
    own_kind: crate::donorkind::DonorKind,
    backup_kind: crate::donorkind::DonorKind,
    projected_permille: i32,
    borrow_underway: bool,
    extracting: bool,
    labelled_same_video: bool,
    backup_mismatched: bool,
) -> bool {
    use crate::donorkind::DonorKind;
    // A backup labelled the same video lends while the extraction runs even when a fleet found
    // it unborrowable by range (its video compressed): zipfast restarts its decoder near the
    // frontier (`restartlend`). One that cannot be restarted is refused by that try and counts
    // as mismatched (`backup_mismatched`), and the download fails over then.
    let backup_lends = matches!(
        backup_kind,
        DonorKind::Twin | DonorKind::Borrowable | DonorKind::EncryptedBorrowable
    ) || (extracting && labelled_same_video)
        || (borrow_underway && backup_kind != DonorKind::Unborrowable);
    // While direct unpack runs, failing over would throw its extraction away and start the
    // backup from its first byte: the download stays however much of it is gone, and every
    // volume it lost is borrowed from the backup, so the extraction carries on with the
    // backup's bytes (a dead volume is handed to it before its borrow finishes, see
    // `streamrepair::offer_dead_fills`). Without a running extraction there is nothing to keep,
    // and a download mostly gone fails over as before.
    let mostly_alive = !dead_rate && projected_permille >= 500;
    !backup_mismatched
        && live_borrowing
        && own_kind != DonorKind::Unborrowable
        && backup_lends
        && (extracting || mostly_alive)
}

/// A primary whose own articles arrive at most this share (permille of those
/// tried) is weak next to a whole copy of the same video.
pub const WEAK_PRIMARY_PERMILLE: i32 = 900;
/// A held copy at least this alive (permille sampled) may replace a weak primary.
pub const SWITCH_BACKUP_ALIVE_PERMILLE: i32 = 975;
/// A weak primary that has downloaded less than this share (permille) of its
/// data fails over to a whole alt instead of borrowing every lost article.
pub const WEAK_SWITCH_MAX_DONE_PERMILLE: i32 = 500;

/// What a fleet primary measured weak does about a held alt of the same video
/// ([`weak_primary_switch`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeakSwitch {
    /// keep it as it is (borrowing what it lacks)
    Stay,
    /// fail over to the alt now, while little of the primary came: the alt
    /// downloads and extracts from its first byte (only when the primary's
    /// extraction is not running or the alt cannot lend live)
    FailOver,
    /// cut the primary at its frontier (`frontiercut`): its extraction goes on,
    /// it fetches none of its own data articles left, and the alt lends every
    /// byte past the frontier (failover-continue), so nothing the primary
    /// downloaded is fetched again
    ContinueAtFrontier,
}

/// The facts [`weak_primary_switch`] decides on.
#[derive(Clone, Copy, Debug, Default)]
pub struct WeakSwitchFacts {
    /// the backup is a copy held in the primary's own fleet
    pub held_in_fleet: bool,
    /// articles of the primary tried so far, and its projected share (permille)
    pub tried: i32,
    pub projected: i32,
    /// the backup's sampled alive share (permille)
    pub backup_alive: i32,
    /// the backup is proven to hold the same video (equal inner keys)
    pub same_video: bool,
    /// the backup is labelled a twin: its articles already stand in one for one
    /// (article fallback, cutover), so it is never failed over to from here
    pub backup_twin: bool,
    /// the primary is held already (failed over a moment ago; its last
    /// articles in flight still report)
    pub primary_held: bool,
    /// the backup is locked (no password for its encrypted headers)
    pub backup_locked: bool,
    /// the share of the primary's data downloaded (permille)
    pub done: i32,
    /// direct unpack already finished and verified the extraction
    pub extraction_verified: bool,
    /// the primary's extraction runs and the alt can lend what lies past its
    /// frontier live (live borrowing on, the alt not refused by a check or a
    /// restart): the switch cuts the primary there instead of failing over
    pub continuable: bool,
    /// the primary is cut at its frontier already
    pub cut: bool,
}

/// Dune on 6790: a 77% primary downloaded for hours, borrowing hole by hole
/// (2.6 times the bytes it recovered), while a held alt of the same video was
/// 100% alive. Asked where the dead-download check reached a doomed or dead
/// primary: once its own articles prove it weak (enough tried, at most
/// [`WEAK_PRIMARY_PERMILLE`] arrived) next to an alt held in its fleet whose
/// video is proven the same (equal inner keys) and at least
/// [`SWITCH_BACKUP_ALIVE_PERMILLE`] alive (and clearly ahead), the alt takes
/// over while less than [`WEAK_SWITCH_MAX_DONE_PERMILLE`] of the primary came.
/// A twin backup is left to the article fallback, which takes its articles one
/// for one already.
pub fn weak_primary_switch(f: &WeakSwitchFacts) -> WeakSwitch {
    let weak = f.held_in_fleet
        && !f.primary_held
        && !f.cut
        && f.same_video
        && !f.backup_twin
        && !f.backup_locked
        && !f.extraction_verified
        && f.tried >= PROJECTED_FAILURE_SAMPLE
        && f.projected >= 0
        && f.projected <= WEAK_PRIMARY_PERMILLE
        && f.backup_alive >= SWITCH_BACKUP_ALIVE_PERMILLE
        && f.backup_alive >= f.projected + 50
        && f.done < WEAK_SWITCH_MAX_DONE_PERMILLE;
    if !weak {
        WeakSwitch::Stay
    } else if f.continuable {
        WeakSwitch::ContinueAtFrontier
    } else {
        WeakSwitch::FailOver
    }
}

/// Whether `backup` is proven to hold the video of `nzb`: equal inner keys
/// (size and CRC32/BLAKE2sp from the archive headers,
/// [`crate::twinfields::same_inner_key`]) on both copies. Nothing else: a
/// label recorded against another copy proves nothing about this one.
#[cfg(unix)]
fn backup_same_video(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
    backup: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    let key = |ptr| host.nzb_get_param(ptr, crate::twinfields::INNER_KEY_PARAM);
    match (key(nzb.ptr), key(backup.ptr)) {
        (Some(a), Some(b)) => {
            crate::twinfields::same_inner_key(&a, &b, crate::twinfields::MIN_KEY_SIZE) == Some(true)
        }
        _ => false,
    }
}

/// Whether `backup` carries any twin label (TwinCheck's `DupeKind=twin`, or
/// the fleet's twin kind): its articles already stand in one for one, so the
/// weak-primary switch leaves it to the article fallback.
#[cfg(unix)]
fn backup_is_twin(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    backup: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    host.nzb_get_param(backup.ptr, b"DupeKind")
        .is_some_and(|k| k.trim_ascii().starts_with(b"twin"))
        || crate::donorkind::DonorKind::from_param(
            host.nzb_get_param(backup.ptr, crate::donorkind::KIND_PARAM).as_deref(),
        ) == crate::donorkind::DonorKind::Twin
}

/// Why `backup` cannot carry on the running (or kept) extraction of `nzb`, which failing
/// over to it throws away (failover-continue: wherever it can, the backup lends the rest
/// instead, see [`alt_covers_lost_articles`]). Asked only while an extraction runs.
fn cannot_continue_reason(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb: &crate::queuesubsys_host::NzbSnap,
    backup: &crate::queuesubsys_host::NzbSnap,
) -> std::borrow::Cow<'static, str> {
    use crate::donorkind::DonorKind;
    let kind = |ptr| {
        DonorKind::from_param(host.nzb_get_param(ptr, crate::donorkind::KIND_PARAM).as_deref())
    };
    if let Some(why) = crate::restartlend::refusal(nzb.id, &backup.queued_filename) {
        // zipfast could not restart its decoder near the frontier (no restart point in its
        // coder, say): the reason the restart gave
        return format!("zipfast could not restart its decoder near the frontier ({why})").into();
    }
    if !live_borrowing(opts) {
        "live borrowing is off (it needs DupeArticleFallback=live, DirectUnpack and no RawArticle)".into()
    } else if crate::postproc::dead_volume_donor_cannot_lend(nzb.id, &backup.queued_filename) {
        "its video failed the check against the download's, or it holds the video compressed".into()
    } else if kind(backup.ptr) == DonorKind::Unborrowable {
        "it holds the video compressed (or encrypted with no password), which live repair cannot lend from".into()
    } else if kind(nzb.ptr) == DonorKind::Unborrowable {
        "the download's own archive cannot be borrowed into".into()
    } else if host
        .nzb_get_param(backup.ptr, b"DupeKind")
        .is_some_and(|k| k.trim_ascii() == b"other")
    {
        "TwinCheck found another video in it".into()
    } else if host
        .nzb_get_param(backup.ptr, b"DupeKind")
        .is_some_and(|k| k.trim_ascii().starts_with(b"twin:"))
    {
        "it is a near twin (only some of its files agree), which proves nothing of the video".into()
    } else if kind(backup.ptr) == DonorKind::Unknown && !backup_may_lend_while_extracting(host, backup.ptr) {
        "nothing shows it holds the same video".into()
    } else {
        "its labels do not show it can lend what lies past the frontier".into()
    }
}

/// Logs why the backup a download fails over to cannot continue its running extraction (see
/// [`cannot_continue_reason`]): it starts from its first byte. Nothing is logged without a
/// running extraction (nothing to continue) or without a reason.
fn note_cannot_continue(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb: &crate::queuesubsys_host::NzbSnap,
    backup: &crate::queuesubsys_host::NzbSnap,
) {
    // failing over ends a frontier cut (`frontiercut`): the download fetches its own articles
    // again if it is ever released
    crate::frontiercut::forget(nzb.id);
    if nzb.direct_unpack_status != DirectUnpackStatus::Running {
        return;
    }
    let why = cannot_continue_reason(host, opts, nzb, backup);
    let mut msg = b"Backup ".to_vec();
    msg.extend_from_slice(&backup.name);
    msg.extend_from_slice(b" cannot continue the extraction of ");
    msg.extend_from_slice(&nzb.name);
    msg.extend_from_slice(b": ");
    msg.extend_from_slice(why.as_bytes());
    msg.extend_from_slice(b"; failing over starts it from its first byte");
    host.nzb_print_message(
        nzb.ptr,
        crate::queuesubsys_host::MK_INFO,
        &crate::releasename::log_name(&msg),
    );
}

/// True while direct unpack of `nzb` runs and `backup` can feed the rest of it (see
/// [`alt_covers_lost_articles`]): failing over would throw that extraction away, so the lost
/// parts are borrowed from the backup instead, whatever the damage.
fn backup_continues_extraction(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb: &crate::queuesubsys_host::NzbSnap,
    backup: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    // a primary cut at its frontier (`frontiercut`) has its alt lend the rest while it can
    if nzb.direct_unpack_status == DirectUnpackStatus::Running
        && crate::frontiercut::cut_for(nzb.id) == Some(backup.id)
        && live_borrowing(opts)
        && crate::restartlend::refusal(nzb.id, &backup.queued_filename).is_none()
        && !backup_cannot_lend(host, nzb, backup)
    {
        return true;
    }
    nzb.direct_unpack_status == DirectUnpackStatus::Running
        && alt_covers_lost_articles(
            true,
            live_borrowing(opts),
            crate::donorkind::DonorKind::from_param(
                host.nzb_get_param(nzb.ptr, crate::donorkind::KIND_PARAM).as_deref(),
            ),
            crate::donorkind::DonorKind::from_param(
                host.nzb_get_param(backup.ptr, crate::donorkind::KIND_PARAM).as_deref(),
            ),
            0,
            crate::postproc::dead_volume_borrow_underway(nzb.id),
            true,
            backup_may_lend_while_extracting(host, backup.ptr),
            backup_cannot_lend(host, nzb, backup),
        )
}

/// Whether `backup_ptr` carries a label that makes keeping a running extraction for it worth it:
/// a twin TwinCheck proved (`DupeKind=twin`), the same video in another container, or another
/// container whose video is not verified yet (`container`; see
/// `donorkind::unknown_kind_is_labelled_alt`). Not proof of the same video: each borrow checks
/// the video before it lends.
pub(crate) fn backup_may_lend_while_extracting(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    backup_ptr: NzbRef,
) -> bool {
    // A copy TwinCheck proved a twin (`twin`: every par2 file agrees) holds the same files, so
    // the same video: it lends whatever its fleet kind (none for a plain dupe-key copy). A near
    // twin (`twin:N/M`, only some files agree) is not a twin and proves nothing of the video.
    if host
        .nzb_get_param(backup_ptr, b"DupeKind")
        .is_some_and(|k| k.trim_ascii() == b"twin")
    {
        return true;
    }
    crate::donorkind::unknown_kind_is_labelled_alt(
        host.nzb_get_param(backup_ptr, crate::donorkind::KIND_PARAM).as_deref(),
        host.nzb_get_param(backup_ptr, b"DupeKind").as_deref(),
        host.nzb_get_param(backup_ptr, crate::dupesearch_coord::VIDEO_PARAM).as_deref(),
    )
}

/// Set while the download next in line waits for its dead-pick probe's connection (see
/// `dupeprobe::probe_waiting_for_connection`): the queue has work left, so it neither idles
/// nor, in standalone mode, ends with the download unfinished.
static PROBE_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static KEPT_FOR_BORROWING: std::sync::Mutex<Option<std::collections::HashSet<i32>>> =
    std::sync::Mutex::new(None);

/// (download, backup) pairs already told they cannot lend (see [`backup_cannot_lend`]).
static TOLD_CANNOT_LEND: std::sync::Mutex<Option<std::collections::HashSet<(i32, i32)>>> =
    std::sync::Mutex::new(None);

/// Forgets that download `nzb_id` was kept for borrowing and what it was told of its backups.
pub(crate) fn forget_kept_for_borrowing(nzb_id: i32) {
    if let Some(g) = KEPT_FOR_BORROWING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        g.remove(&nzb_id);
    }
    if let Some(g) = TOLD_CANNOT_LEND.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        g.retain(|&(id, _)| id != nzb_id);
    }
}

/// Logs "keeping for borrowing" once per download.
fn note_kept_for_borrowing(nzb_id: i32) -> bool {
    let mut g = KEPT_FOR_BORROWING.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(Default::default).insert(nzb_id)
}

/// True while download `nzb_id` is kept to borrow its lost parts (see
/// [`note_kept_for_borrowing`]).
fn was_kept_for_borrowing(nzb_id: i32) -> bool {
    KEPT_FOR_BORROWING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|g| g.contains(&nzb_id))
}

/// What [`weak_primary_switch`] decides on for download `nzb` and its best backup, with the
/// dead-download check's `tried` articles and `projected` health.
#[cfg(unix)]
fn weak_switch_facts(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb: &crate::queuesubsys_host::NzbSnap,
    backup_nzb: &crate::queuesubsys_host::NzbSnap,
    tried: i32,
    projected: i32,
) -> WeakSwitchFacts {
    WeakSwitchFacts {
        held_in_fleet: crate::fleethold::holds_in_queue(host, nzb)
            && crate::fleethold::is_held(host, backup_nzb.ptr)
            && backup_nzb.dupe_key.eq_ignore_ascii_case(&nzb.dupe_key),
        tried,
        projected,
        backup_alive: backup_alive_permille(host, backup_nzb.ptr),
        same_video: backup_same_video(host, nzb, backup_nzb),
        backup_twin: backup_is_twin(host, backup_nzb),
        primary_held: crate::fleethold::is_held(host, nzb.ptr),
        backup_locked: crate::dupecoord::locked_host(host, backup_nzb.ptr),
        done: data_done_permille(nzb),
        extraction_verified: direct_unpack_verified(nzb.direct_unpack_status),
        // (a cut is kept only for the alt it was made for: another copy the failover order
        // picks later was never proven to hold the video)
        continuable: nzb.direct_unpack_status == DirectUnpackStatus::Running
            && crate::frontiercut::cut_for(nzb.id).is_none_or(|alt| alt == backup_nzb.id)
            && live_borrowing(opts)
            && !crate::postproc::dead_volume_donor_cannot_lend(nzb.id, &backup_nzb.queued_filename)
            && crate::restartlend::refusal(nzb.id, &backup_nzb.queued_filename).is_none(),
        cut: crate::frontiercut::is_cut(nzb.id),
    }
}

/// A download the dead-download check leaves alone: not an nzb, already being removed or parked,
/// or one whose direct unpack already finished and verified. Failing that one over (on projected
/// health, as nest3v on 6790 did 1 s after its verified extraction) only downloads and extracts
/// the release again.
fn dead_check_skipped(nzb: &crate::queuesubsys_host::NzbSnap) -> bool {
    nzb.kind != crate::queuesubsys_host::NK_NZB
        || nzb.deleting
        || nzb.parking
        || nzb.delete_status != DeleteStatus::None
        || direct_unpack_verified(nzb.direct_unpack_status)
}

/// Direct unpack finished and verified the extraction.
fn direct_unpack_verified(status: DirectUnpackStatus) -> bool {
    status == DirectUnpackStatus::Success
}

/// True when the live dead-volume borrow of download `nzb` found the video of `backup`
/// to differ and no other donor lends the dead volumes (see
/// `postproc::dead_volume_donor_cannot_lend`). Says so once.
fn backup_cannot_lend(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
    backup: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    if !crate::postproc::dead_volume_donor_cannot_lend(nzb.id, &backup.queued_filename)
        && crate::restartlend::refusal(nzb.id, &backup.queued_filename).is_none()
    {
        return false;
    }
    let was_kept = was_kept_for_borrowing(nzb.id);
    let first = TOLD_CANNOT_LEND
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(Default::default)
        .insert((nzb.id, backup.id));
    if first {
        let mut msg = if was_kept { b"No longer keeping ".to_vec() } else { b"Not keeping ".to_vec() };
        msg.extend_from_slice(&nzb.name);
        msg.extend_from_slice(b" for borrowing: duplicate ");
        msg.extend_from_slice(&backup.name);
        match crate::restartlend::refusal(nzb.id, &backup.queued_filename) {
            // its video is compressed and zipfast found no restart near the frontier: no word
            // on whether it is the same video
            Some(why) => msg.extend_from_slice(
                format!(" cannot lend its dead volumes: zipfast could not restart its decoder near the frontier ({why})")
                    .as_bytes(),
            ),
            None => msg.extend_from_slice(
                format!(
                    " cannot lend its dead volumes (in {} borrow attempts the video inside did not match, or was compressed, which live repair cannot lend from)",
                    crate::postproc::DEAD_VOLUME_DONOR_MISMATCH_RUNS
                )
                .as_bytes(),
            ),
        }
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
    }
    true
}

/// `QueueCoordinator::SideFile` (`QueueCoordinator.cpp`).
pub fn is_side_file(filename: &[u8], ext_cleanup_disk: &[u8]) -> bool {
    let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') else {
        return false;
    };
    let extension = &filename[dot_pos..];
    const BUILTIN_SIDES: &[&[u8]] = &[
        b".nfo", b".sfv", b".txt", b".url", b".srr", b".srs", b".nzb", b".jpg", b".jpeg", b".png",
    ];
    for &side in BUILTIN_SIDES {
        if strcasecmp_bytes(extension, side) {
            return true;
        }
    }
    for token in ext_cleanup_disk.split(|&b| b == b',' || b == b';' || b == b' ') {
        if token.is_empty() {
            continue;
        }
        if strcasecmp_bytes(extension, token) && !strcasecmp_bytes(token, b".par2") {
            return true;
        }
    }
    false
}

/// `QueueCoordinator::ResetHangingDownloads` (`QueueCoordinator.cpp`).
#[inline]
pub fn is_hanging_download(
    now: i64,
    last_update: i64,
    article_timeout: i32,
    is_running: bool,
) -> bool {
    is_running && article_timeout > 0 && (now - last_update) > (article_timeout as i64) + 1
}

/// Tie-breaking comparison for duplicate file sizes and indices (`QueueCoordinator::CheckDupeFileInfos`).
#[inline]
pub fn is_smaller_dupe_file(size1: i64, idx1: i32, size2: i64, idx2: i32) -> bool {
    size1 < size2 || (size1 == size2 && idx2 < idx1)
}

/// Identifies indices of files in `filenames` (`0..filenames.len()`) whose filename
/// appears exactly twice and which lose the size/index tie-breaker
/// (`QueueCoordinator::CheckDupeFileInfos` in `daemon/queue/QueueCoordinator.cpp`).
pub fn find_dupe_file_indices(
    filenames: &[&[u8]],
    sizes: &[i64],
    out_dupe_indices: &mut [usize],
) -> usize {
    let n = filenames.len().min(sizes.len());
    let mut by_name: std::collections::HashMap<&[u8], Vec<(i32, usize)>> =
        std::collections::HashMap::with_capacity(n);
    for (idx, &name) in filenames.iter().enumerate().take(n) {
        by_name
            .entry(name)
            .or_default()
            .push(((idx + 1) as i32, idx));
    }
    let mut written = 0usize;
    for (idx, &name) in filenames.iter().enumerate().take(n) {
        let Some(same) = by_name.get(name) else {
            continue;
        };
        if same.len() != 2 {
            continue;
        }
        let (self_pos, self_idx) = if same[0].1 == idx { same[0] } else { same[1] };
        let (other_pos, other_idx) = if same[0].1 == idx { same[1] } else { same[0] };
        if is_smaller_dupe_file(sizes[self_idx], self_pos, sizes[other_idx], other_pos)
            && written < out_dupe_indices.len()
        {
            out_dupe_indices[written] = idx;
            written += 1;
        }
    }
    written
}

/// Computes `(posted_min_ago, start_in_min)` for the `PropagationDelay` log message in `QueueCoordinator::GetNextArticle`.
#[inline]
pub fn calc_propagation_wait_log_mins(
    file_time: i64,
    propagation_delay: i32,
    cur_date: i64,
) -> (i32, i32) {
    let wait_sec = (file_time as i32)
        .wrapping_add(propagation_delay)
        .wrapping_sub(cur_date as i32);
    let posted_min = (cur_date.wrapping_sub(file_time) as i32) / 60;
    let start_min = (wait_sec / 60).wrapping_add(1);
    (posted_min, start_min)
}

/// Applies article completion success counters to file and NZB counters in `QueueCoordinator::ArticleCompleted`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn apply_success_article_counters(
    article_size: i64,
    is_par_file: bool,
    file_success_size: i64,
    nzb_cur_success_size: i64,
    nzb_par_cur_success_size: i64,
    file_success_articles: i32,
    nzb_cur_success_articles: i32,
) -> (i64, i64, i64, i32, i32) {
    let par_add = if is_par_file { article_size } else { 0 };
    (
        file_success_size.wrapping_add(article_size),
        nzb_cur_success_size.wrapping_add(article_size),
        nzb_par_cur_success_size.wrapping_add(par_add),
        file_success_articles.wrapping_add(1),
        nzb_cur_success_articles.wrapping_add(1),
    )
}

/// Updates the file and NZB failed-article counters in `QueueCoordinator::ArticleCompleted`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn apply_failed_article_counters(
    part_number: i32,
    par_file: i32,
    article_size: i32,
    dupe_failed_run: i32,
    file_failed_size: i64,
    nzb_cur_failed_size: i64,
    nzb_par_cur_failed_size: i64,
    file_failed_articles: i32,
    nzb_cur_failed_articles: i32,
) -> (i32, i64, i64, i64, i32, i32) {
    let next_dupe_failed_run = if part_number != 1 && par_file == 0 {
        dupe_failed_run.wrapping_add(1)
    } else {
        dupe_failed_run
    };
    let size64 = article_size as i64;
    let next_file_failed_size = file_failed_size.wrapping_add(size64);
    let next_nzb_cur_failed_size = nzb_cur_failed_size.wrapping_add(size64);
    let next_nzb_par_cur_failed_size =
        nzb_par_cur_failed_size.wrapping_add(if par_file != 0 { size64 } else { 0 });
    let next_file_failed_articles = file_failed_articles.wrapping_add(1);
    let next_nzb_cur_failed_articles = nzb_cur_failed_articles.wrapping_add(1);
    (
        next_dupe_failed_run,
        next_file_failed_size,
        next_nzb_cur_failed_size,
        next_nzb_par_cur_failed_size,
        next_file_failed_articles,
        next_nzb_cur_failed_articles,
    )
}

/// Selects the duplicate donor ID for a completed article in `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn select_dupe_donor_id(
    dupe_fallback_round: i32,
    donors: &[i32],
    msg_id: &[u8],
    orig_msg_id: &[u8],
) -> i32 {
    let source = dupe_fallback_round.wrapping_sub(1);
    if source >= 0 && (source as usize) < donors.len() && msg_id != orig_msg_id {
        donors[source as usize]
    } else {
        0
    }
}

/// Evaluates whether a finished article counts as a proven duplicate recovery and whether `DupeCutover` should be triggered.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn eval_dupe_recovery_and_cutover(
    dupe_fallback_round: i32,
    dupe_proactive: bool,
    msg_id: &[u8],
    orig_msg_id: &[u8],
    cur_file_recovered: i32,
    cur_nzb_recovered: i32,
    dupe_cutover: bool,
    cutover_threshold: i32,
) -> (bool, i32, i32, bool) {
    let recovered = dupe_fallback_round > 0
        && !dupe_proactive
        && !orig_msg_id.is_empty()
        && msg_id != orig_msg_id;
    if !recovered {
        return (false, cur_file_recovered, cur_nzb_recovered, false);
    }
    let next_file_rec = cur_file_recovered.wrapping_add(1);
    let next_nzb_rec = cur_nzb_recovered.wrapping_add(1);
    let trigger_cutover = !dupe_cutover && next_file_rec >= cutover_threshold;
    (true, next_file_rec, next_nzb_rec, trigger_cutover)
}

/// Advances remaining sizes and completed article count when an article finishes without retry in `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn advance_article_completion_counters(
    rem_file_size: i64,
    rem_nzb_size: i64,
    paused_size: i64,
    article_size: i32,
    is_paused: bool,
    completed_articles: i32,
    total_articles: i32,
) -> (i64, i64, i64, i32, bool) {
    let sz = i64::from(article_size);
    let new_rem_file = rem_file_size.wrapping_sub(sz);
    let new_rem_nzb = rem_nzb_size.wrapping_sub(sz);
    let new_paused = if is_paused {
        paused_size.wrapping_sub(sz)
    } else {
        paused_size
    };
    let new_completed = completed_articles.wrapping_add(1);
    let file_completed = total_articles == new_completed;
    (
        new_rem_file,
        new_rem_nzb,
        new_paused,
        new_completed,
        file_completed,
    )
}

/// Evaluates `(file_completed, complete_file_parts)` after health and dead-download checks in `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn eval_file_completion_and_assembly(
    file_completed: bool,
    parking: bool,
    active_downloads: i32,
    dupe_deleted: bool,
    file_deleted: bool,
    finished_before_cancel: bool,
) -> (bool, bool) {
    let fc = if parking && active_downloads == 1 && !dupe_deleted {
        true
    } else {
        file_completed
    };
    let complete_parts = fc && (!file_deleted || parking || finished_before_cancel);
    (fc, complete_parts)
}

/// Applies counter adjustments when `CompleteFileParts()` fails in `QueueCoordinator::ArticleCompleted`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn apply_assembly_failure_counters(
    success_articles: i32,
    success_size: i64,
    is_par_file: bool,
    inout_file_failed_articles: &mut i32,
    inout_file_failed_size: &mut i64,
    inout_nzb_cur_failed_articles: &mut i32,
    inout_nzb_cur_success_articles: &mut i32,
    inout_nzb_cur_failed_size: &mut i64,
    inout_nzb_cur_success_size: &mut i64,
    inout_nzb_par_cur_failed_size: &mut i64,
    inout_nzb_par_cur_success_size: &mut i64,
) {
    *inout_file_failed_articles = inout_file_failed_articles.wrapping_add(success_articles);
    *inout_file_failed_size = inout_file_failed_size.wrapping_add(success_size);
    *inout_nzb_cur_failed_articles = inout_nzb_cur_failed_articles.wrapping_add(success_articles);
    *inout_nzb_cur_success_articles = inout_nzb_cur_success_articles
        .wrapping_sub(success_articles)
        .max(0);
    *inout_nzb_cur_failed_size = inout_nzb_cur_failed_size.wrapping_add(success_size);
    *inout_nzb_cur_success_size = inout_nzb_cur_success_size.wrapping_sub(success_size).max(0);
    if is_par_file {
        *inout_nzb_par_cur_failed_size = inout_nzb_par_cur_failed_size.wrapping_add(success_size);
        *inout_nzb_par_cur_success_size = inout_nzb_par_cur_success_size
            .wrapping_sub(success_size)
            .max(0);
    }
}

/// Deduplicates, sorts, and formats a comma-separated list of donor IDs (`QueueCoordinator::RejectBorrowedArticles`).
pub fn format_donor_id_list(donor_ids: &[i32]) -> (Vec<i32>, Vec<u8>) {
    let mut unique: Vec<i32> = donor_ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let mut out = String::new();
    for (i, id) in unique.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&id.to_string());
    }
    (unique, out.into_bytes())
}

/// Evaluates `(delete_data, save_partial)` flags for `DiskState` operations in `QueueCoordinator::DeleteFileInfo`.
#[inline]
pub fn eval_delete_file_diskstate_flags(
    file_status: i32,
    cf_success: i32,
    cf_partial: i32,
    file_deleted: i32,
    parking: i32,
    completed: i32,
) -> (bool, bool) {
    let delete_data = file_status == cf_success || (file_deleted != 0 && parking == 0);
    let save_partial = file_status == cf_partial && (completed != 0 || parking != 0);
    (delete_data, save_partial)
}

/// Demotes a finished article's counters back to failed in `QueueCoordinator::CountAsFailed`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn demote_article_counters(
    article_size: i32,
    is_par_file: bool,
    dupe_proactive: bool,
    msg_id: &[u8],
    orig_msg_id: &[u8],
    inout_file_success_size: &mut i64,
    inout_file_failed_size: &mut i64,
    inout_file_success_articles: &mut i32,
    inout_file_failed_articles: &mut i32,
    inout_nzb_cur_success_size: &mut i64,
    inout_nzb_cur_failed_size: &mut i64,
    inout_nzb_par_cur_success_size: &mut i64,
    inout_nzb_par_cur_failed_size: &mut i64,
    inout_nzb_cur_success_articles: &mut i32,
    inout_nzb_cur_failed_articles: &mut i32,
    inout_file_dupe_recovered: &mut i32,
    inout_nzb_dupe_recovered: &mut i32,
) {
    let sz = i64::from(article_size);
    *inout_file_success_size = inout_file_success_size.wrapping_sub(sz);
    *inout_file_failed_size = inout_file_failed_size.wrapping_add(sz);
    *inout_file_success_articles = inout_file_success_articles.wrapping_sub(1);
    *inout_file_failed_articles = inout_file_failed_articles.wrapping_add(1);
    *inout_nzb_cur_success_size = inout_nzb_cur_success_size.wrapping_sub(sz);
    *inout_nzb_cur_failed_size = inout_nzb_cur_failed_size.wrapping_add(sz);
    if is_par_file {
        *inout_nzb_par_cur_success_size = inout_nzb_par_cur_success_size.wrapping_sub(sz);
        *inout_nzb_par_cur_failed_size = inout_nzb_par_cur_failed_size.wrapping_add(sz);
    }
    *inout_nzb_cur_success_articles = inout_nzb_cur_success_articles.wrapping_sub(1);
    *inout_nzb_cur_failed_articles = inout_nzb_cur_failed_articles.wrapping_add(1);

    if !dupe_proactive && !orig_msg_id.is_empty() && msg_id != orig_msg_id {
        *inout_file_dupe_recovered = inout_file_dupe_recovered.wrapping_sub(1);
        *inout_nzb_dupe_recovered = inout_nzb_dupe_recovered.wrapping_sub(1);
    }
}

/// Evaluates whether `QueueCoordinator::CheckHealth` should return early without taking action.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn should_skip_health_check(
    health_check: i32,
    hc_none: i32,
    hc_park: i32,
    par_scan: i32,
    ps_dupe: i32,
    health_paused: bool,
    delete_status: DeleteStatus,
    health: i32,
    critical_health: i32,
    success_articles: i32,
    total_articles: i32,
) -> bool {
    health_check == hc_none
        || health_paused
        || delete_status == DeleteStatus::Health
        || health >= critical_health
        || (par_scan == ps_dupe
            && health_check == hc_park
            && total_articles > 0
            && i64::from(success_articles) * 100 / i64::from(total_articles) > 10)
}

/// Evaluates whether `other` is a size twin of `nzbInfo` (`QueueCoordinator::HasSizeTwin`).
pub fn is_size_twin(
    is_self: bool,
    same_name_or_key: bool,
    self_size: i64,
    other_size: i64,
    other_par_size: i64,
    largest_self_file: i64,
    other_file_sizes: &[i64],
) -> bool {
    if is_self || !same_name_or_key {
        return false;
    }
    if size_near(self_size, other_size)
        || (other_par_size > 0 && size_near(self_size, other_size - other_par_size))
    {
        return true;
    }
    other_file_sizes
        .iter()
        .any(|&fs| size_near(largest_self_file, fs))
}

/// Evaluates whether a queued duplicate entry is considered active (`QueueCoordinator::HasActiveDupe`).
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn is_queued_dupe_active(
    deleting: bool,
    delete_status: DeleteStatus,
    active_downloads: i32,
    has_post_info: bool,
    post_working: bool,
    post_stage: PostStage,
    extra_priority: bool,
    self_completed_empty: bool,
    direct_unpack_status: DirectUnpackStatus,
) -> bool {
    if (deleting || delete_status != DeleteStatus::None) && active_downloads == 0 {
        return false;
    }
    let is_active_download = active_downloads > 0;
    let is_post_processing = has_post_info
        && (post_working
            || post_stage != PostStage::Queued
            || extra_priority
            || self_completed_empty);
    let is_direct_unpacking = direct_unpack_status == DirectUnpackStatus::Running;
    is_active_download || is_post_processing || is_direct_unpacking
}

/// Whether a copy in post-processing lets the other copies of its duplicate key download: with
/// HealthCheck=dupe (or park), one that is stream-repairing its lost articles from duplicates
/// does. The repair can take long (a slow duplicate), and a copy downloading whole meanwhile
/// may finish first. Only one of them goes on past the repair to unpack and move: the post
/// stages after it are gated by duplicate key (`prepost::dupe_post_gate`), and the copy that
/// finishes on disk first stops the other, which stays in history as a backup (see
/// `dupecoord::nzb_completed_host`).
///
/// Only when both copies download into folders of their own under InterDir (`repairing_dir`,
/// `own_dir`; `opts` gives the health check and InterDir). Without InterDir a copy downloads
/// into its final folder: under the same name and category that is the repairing copy's
/// folder, and with DupeCheck its files would be taken for duplicates of those and discarded;
/// under another name both copies' files would end up in the destination.
pub fn repair_lets_other_copies_download(
    has_post_info: bool,
    post_stage: PostStage,
    repairing_dir: &[u8],
    own_dir: &[u8],
    opts: impl FnOnce() -> (i32, Vec<u8>),
) -> bool {
    if !has_post_info || post_stage != PostStage::StreamRepairing {
        return false;
    }
    let (health_check, inter_dir) = opts();
    let sep = crate::paths::PATH_SEPARATOR;
    matches!(
        health_check,
        crate::queuesubsys_host::HC_DUPE | crate::queuesubsys_host::HC_PARK
    ) && repairing_dir != own_dir
        && crate::unpack::use_inter_dir(repairing_dir, &inter_dir, sep)
        && crate::unpack::use_inter_dir(own_dir, &inter_dir, sep)
}

/// Determines whether `QueueCoordinator::CheckDupeFailover` should defer failover and whether `DupeFailoverChecked` should be updated.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn should_defer_dupe_failover(
    fallback_enabled: bool,
    attempted: i32,
    recovered: i32,
    failover_sample: i32,
    defer_state: i32,
    dp_deferred: i32,
    par_cannot_cover: bool,
    failed_articles: i32,
    failover_checked: i32,
) -> (bool, bool) {
    if fallback_enabled
        && attempted > 0
        && (attempted < failover_sample || recovered.saturating_mul(2) >= attempted)
    {
        return (true, false);
    }
    if fallback_enabled && attempted == 0 && defer_state == dp_deferred && !par_cannot_cover {
        return (true, false);
    }
    if failover_checked >= 0 && failed_articles.wrapping_sub(failover_checked) < failover_sample {
        return (true, false);
    }
    (false, true)
}

/// Determines whether `QueueCoordinator::CheckDeadDownload` should park an unhealthy download that has no backup in history.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn should_park_dead_without_backup(
    fallback_enabled: bool,
    defer_state: i32,
    dp_deferred: i32,
    attempted: i32,
    recovered: i32,
    tried: i32,
    projected_sample: i32,
    files_tried: i32,
    far_below: bool,
    below: bool,
) -> bool {
    let borrowing = fallback_enabled
        && (defer_state == dp_deferred
            || (attempted > 0 && recovered.saturating_mul(2) >= attempted));
    tried >= projected_sample && files_tried >= 3 && (far_below || below) && !borrowing
}

/// Determines whether `QueueCoordinator::CheckParDamage` should wait for duplicate article borrowing before failing over.
///
/// It waits for `failover_sample` lost articles tried for borrowing only while that many can
/// still come (`untried`: the lost articles expected among the data articles left, see
/// [`expected_lost`]): lost articles count only once the par2
/// damage is known (before, they wait for par-check), so a download whose damage turned up
/// late may never reach the sample, and it would download to its end and fail par-repair
/// before any failover (functional `pardamageborrow` with 4 connections counted 30 to 35 of
/// its 48 lost articles). It then judges on what it counted, but never on fewer than half the
/// sample: one or two failed borrows (a donor not read yet, say) are no reason to throw away a
/// download that is nearly done.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn should_wait_for_borrowing_on_par_damage(
    fallback_enabled: bool,
    raw_article: bool,
    dupe_mode: DupeMode,
    has_par2: bool,
    defer_state: i32,
    dp_deferred: i32,
    attempted: i32,
    failover_sample: i32,
    recovered: i32,
    untried: i32,
) -> bool {
    if fallback_enabled && !raw_article && dupe_mode != DupeMode::Force && has_par2 {
        defer_state == dp_deferred
            || (attempted < failover_sample
                && (attempted < failover_sample / 2
                    || attempted.saturating_add(untried.max(0)) >= failover_sample))
            || recovered.saturating_mul(2) >= attempted
    } else {
        false
    }
}

/// How many of `untried` articles are expected to be lost, at the rate the `tried_ok` +
/// `tried_failed` articles so far were (rounded down: a lost article is expected only once a
/// whole one is; all of them before anything was tried). Articles a duplicate lent count as
/// arrived, so the rate is low while borrowing works partly: the failover comes earlier, still
/// judged on at least half the sample.
pub fn expected_lost(untried: i32, tried_ok: i32, tried_failed: i32) -> i32 {
    let tried = i64::from(tried_ok.max(0)) + i64::from(tried_failed.max(0));
    if tried == 0 {
        return untried.max(0);
    }
    let lost = i64::from(untried.max(0)) * i64::from(tried_failed.max(0));
    (lost / tried) as i32
}

/// Formats PAR damage summary string for `QueueCoordinator::CheckParDamage`.
pub fn format_par_damage_summary(
    certain: bool,
    damaged_blocks: i32,
    projected_blocks: i32,
    block_size: i64,
    recovery_blocks: i32,
) -> Vec<u8> {
    let blocks = if certain {
        damaged_blocks
    } else {
        projected_blocks
    };
    let mb = (block_size as f64) / 1024.0 / 1024.0;
    let suffix = if certain { "" } else { " at the rate so far" };
    format!(
        "{blocks} par2 block(s) of {mb:.1} MB damaged{suffix}, its par2-files hold {recovery_blocks} recovery block(s)"
    )
    .into_bytes()
}

/// Evaluates `(discard_par_probe_article, discard_article_infos)` in `QueueCoordinator::DiscardDirectRename`.
#[inline]
pub fn eval_discard_direct_rename_file(
    is_par_file: bool,
    completed_articles: i32,
    active_downloads: i32,
    server_mode: bool,
    has_articles: bool,
    continue_partial: bool,
    cached_articles: i32,
) -> (bool, bool) {
    let discard_par = is_par_file && completed_articles == 1 && active_downloads == 0;
    let discard_infos = server_mode
        && has_articles
        && continue_partial
        && active_downloads == 0
        && cached_articles == 0;
    (discard_par, discard_infos)
}

/// Validates a file entry in `QueueCoordinator::SplitQueueEntries`.
/// Returns `0` if valid, `1` if already (partially) downloaded, `2` if in post-process stage, `3` if from a different collection.
#[inline]
pub fn validate_split_file_state(
    active_downloads: i32,
    completed_articles: i32,
    has_post_info: bool,
    same_nzb: bool,
) -> i32 {
    if active_downloads > 0 || completed_articles > 0 {
        1
    } else if has_post_info {
        2
    } else if !same_nzb {
        3
    } else {
        0
    }
}

/// Counter bundle for `QueueCoordinator::MergeQueueEntries` and `QueueCoordinator::SplitQueueEntries`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NzbgetRsNzbMergeSplitCounters {
    pub file_count: i32,
    pub active_downloads: i32,
    pub full_content_hash: u32,
    pub filtered_content_hash: u32,
    pub size: i64,
    pub remaining_size: i64,
    pub paused_file_count: i32,
    pub paused_size: i64,
    pub success_size: i64,
    pub current_success_size: i64,
    pub failed_size: i64,
    pub current_failed_size: i64,
    pub par_size: i64,
    pub par_success_size: i64,
    pub par_current_success_size: i64,
    pub par_failed_size: i64,
    pub par_current_failed_size: i64,
    pub remaining_par_count: i32,
    pub total_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub current_success_articles: i32,
    pub current_failed_articles: i32,
    pub min_time: i64,
    pub max_time: i64,
    pub downloaded_size: i64,
    pub download_sec: i32,
    pub download_start_time: i64,
}

/// File counter snapshot for `QueueCoordinator::SplitQueueEntries`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NzbgetRsSplitFileCounters {
    pub size: i64,
    pub remaining_size: i64,
    pub success_size: i64,
    pub failed_size: i64,
    pub missed_size: i64,
    pub total_articles: i32,
    pub missed_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub par_file: i32,
    pub paused: i32,
}

/// Accumulates `src` counters into `dest` for `QueueCoordinator::MergeQueueEntries`.
pub fn merge_counters(
    dest: &mut NzbgetRsNzbMergeSplitCounters,
    src: &NzbgetRsNzbMergeSplitCounters,
) {
    dest.file_count = dest.file_count.wrapping_add(src.file_count);
    dest.active_downloads = dest.active_downloads.wrapping_add(src.active_downloads);
    dest.full_content_hash = 0;
    dest.filtered_content_hash = 0;

    dest.size = dest.size.wrapping_add(src.size);
    dest.remaining_size = dest.remaining_size.wrapping_add(src.remaining_size);
    dest.paused_file_count = dest.paused_file_count.wrapping_add(src.paused_file_count);
    dest.paused_size = dest.paused_size.wrapping_add(src.paused_size);

    dest.success_size = dest.success_size.wrapping_add(src.success_size);
    dest.current_success_size = dest
        .current_success_size
        .wrapping_add(src.current_success_size);
    dest.failed_size = dest.failed_size.wrapping_add(src.failed_size);
    dest.current_failed_size = dest
        .current_failed_size
        .wrapping_add(src.current_failed_size);

    dest.par_size = dest.par_size.wrapping_add(src.par_size);
    dest.par_success_size = dest.par_success_size.wrapping_add(src.par_success_size);
    dest.par_current_success_size = dest
        .par_current_success_size
        .wrapping_add(src.par_current_success_size);
    dest.par_failed_size = dest.par_failed_size.wrapping_add(src.par_failed_size);
    dest.par_current_failed_size = dest
        .par_current_failed_size
        .wrapping_add(src.par_current_failed_size);
    dest.remaining_par_count = dest
        .remaining_par_count
        .wrapping_add(src.remaining_par_count);

    dest.total_articles = dest.total_articles.wrapping_add(src.total_articles);
    dest.success_articles = dest.success_articles.wrapping_add(src.success_articles);
    dest.failed_articles = dest.failed_articles.wrapping_add(src.failed_articles);
    dest.current_success_articles = dest
        .current_success_articles
        .wrapping_add(src.current_success_articles);
    dest.current_failed_articles = dest
        .current_failed_articles
        .wrapping_add(src.current_failed_articles);

    if src.min_time < dest.min_time {
        dest.min_time = src.min_time;
    }
    if src.max_time > dest.max_time {
        dest.max_time = src.max_time;
    }

    dest.downloaded_size = dest.downloaded_size.wrapping_add(src.downloaded_size);
    dest.download_sec = dest.download_sec.wrapping_add(src.download_sec);
    if !((dest.download_start_time > 0 && dest.download_start_time < src.download_start_time)
        || src.download_start_time == 0)
    {
        dest.download_start_time = src.download_start_time;
    }
}

/// Transfers one file's counters from `src` to `dst` in `QueueCoordinator::SplitQueueEntries`.
pub fn split_transfer_file(
    src: &mut NzbgetRsNzbMergeSplitCounters,
    dst: &mut NzbgetRsNzbMergeSplitCounters,
    f: &NzbgetRsSplitFileCounters,
) {
    let failed_and_missed_size = f.failed_size.wrapping_add(f.missed_size);
    let failed_and_missed_articles = f.failed_articles.wrapping_add(f.missed_articles);

    src.file_count = src.file_count.wrapping_sub(1);
    src.size = src.size.wrapping_sub(f.size);
    src.remaining_size = src.remaining_size.wrapping_sub(f.remaining_size);
    src.current_success_size = src.current_success_size.wrapping_sub(f.success_size);
    src.current_failed_size = src.current_failed_size.wrapping_sub(failed_and_missed_size);
    src.total_articles = src.total_articles.wrapping_sub(f.total_articles);
    src.failed_articles = src.failed_articles.wrapping_sub(f.missed_articles);
    src.current_success_articles = src
        .current_success_articles
        .wrapping_sub(f.success_articles);
    src.current_failed_articles = src
        .current_failed_articles
        .wrapping_sub(failed_and_missed_articles);

    dst.file_count = dst.file_count.wrapping_add(1);
    dst.size = dst.size.wrapping_add(f.size);
    dst.remaining_size = dst.remaining_size.wrapping_add(f.remaining_size);
    dst.current_success_size = dst.current_success_size.wrapping_add(f.success_size);
    dst.current_failed_size = dst.current_failed_size.wrapping_add(failed_and_missed_size);
    dst.total_articles = dst.total_articles.wrapping_add(f.total_articles);
    dst.failed_articles = dst.failed_articles.wrapping_add(f.missed_articles);
    dst.current_success_articles = dst
        .current_success_articles
        .wrapping_add(f.success_articles);
    dst.current_failed_articles = dst
        .current_failed_articles
        .wrapping_add(failed_and_missed_articles);

    if f.par_file != 0 {
        src.par_size = src.par_size.wrapping_sub(f.size);
        src.par_current_success_size = src.par_current_success_size.wrapping_sub(f.success_size);
        src.par_current_failed_size = src
            .par_current_failed_size
            .wrapping_sub(failed_and_missed_size);
        src.remaining_par_count = src.remaining_par_count.wrapping_sub(1);

        dst.par_size = dst.par_size.wrapping_add(f.size);
        dst.par_current_success_size = dst.par_current_success_size.wrapping_add(f.success_size);
        dst.par_current_failed_size = dst
            .par_current_failed_size
            .wrapping_add(failed_and_missed_size);
        dst.remaining_par_count = dst.remaining_par_count.wrapping_add(1);
    }

    if f.paused != 0 {
        src.paused_file_count = src.paused_file_count.wrapping_sub(1);
        src.paused_size = src.paused_size.wrapping_sub(f.remaining_size);

        dst.paused_file_count = dst.paused_file_count.wrapping_add(1);
        dst.paused_size = dst.paused_size.wrapping_add(f.remaining_size);
    }
}

/// Computes the updated NZB and file counters when discarding downloaded articles
/// in `QueueCoordinator::DiscardDownloadedArticles`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn discard_downloaded_articles_counters(
    nzb_rem_size: i64,
    nzb_paused_size: i64,
    nzb_cur_succ_art: i32,
    nzb_cur_succ_size: i64,
    nzb_par_cur_succ_size: i64,
    nzb_cur_fail_art: i32,
    nzb_cur_fail_size: i64,
    nzb_par_cur_fail_size: i64,
    file_paused: i32,
    file_succ_art: i32,
    file_succ_size: i64,
    file_fail_art: i32,
    file_fail_size: i64,
    file_size: i64,
    file_missed_size: i64,
) -> (i64, i64, i32, i64, i64, i32, i64, i64, i64) {
    let delta_size = file_succ_size.wrapping_add(file_fail_size);
    let next_nzb_rem_size = nzb_rem_size.wrapping_add(delta_size);
    let next_nzb_paused_size = if file_paused != 0 {
        nzb_paused_size.wrapping_add(delta_size)
    } else {
        nzb_paused_size
    };
    let next_nzb_cur_succ_art = nzb_cur_succ_art.wrapping_sub(file_succ_art);
    let next_nzb_cur_succ_size = nzb_cur_succ_size.wrapping_sub(file_succ_size);
    let next_nzb_par_cur_succ_size = nzb_par_cur_succ_size.wrapping_sub(file_succ_size);
    let next_nzb_cur_fail_art = nzb_cur_fail_art.wrapping_sub(file_fail_art);
    let next_nzb_cur_fail_size = nzb_cur_fail_size.wrapping_sub(file_fail_size);
    let next_nzb_par_cur_fail_size = nzb_par_cur_fail_size.wrapping_sub(file_fail_size);
    let next_file_rem_size = file_size.wrapping_sub(file_missed_size);
    (
        next_nzb_rem_size,
        next_nzb_paused_size,
        next_nzb_cur_succ_art,
        next_nzb_cur_succ_size,
        next_nzb_par_cur_succ_size,
        next_nzb_cur_fail_art,
        next_nzb_cur_fail_size,
        next_nzb_par_cur_fail_size,
        next_file_rem_size,
    )
}

#[cfg(unix)]
const DUPE_FAILOVER_SAMPLE: i32 = 32;
#[cfg(unix)]
const BACKUP_LEAD: i32 = 100;

/// How long a backup that just reached history may stay of unknown kind before a download
/// that only looks doomed fails over to it: classifying it (a nested alt, say) takes a few
/// seconds, and failing over first throws away an extraction the backup could have fed.
const BACKUP_CLASSIFY_GRACE_SEC: i64 = 120;

/// Whether a backup parked at `parked_at` (Unix seconds) is still too new to tell whether it
/// lends: its kind is unknown and it arrived under [`BACKUP_CLASSIFY_GRACE_SEC`] ago, or, whatever
/// its kind and age, the fleet intake is still `checking` its video (a nested alt can take
/// longer to open down to it). That check is bounded on its own: it reads until its deadline,
/// then lets go.
///
/// The grace for an unknown kind is only worth its wait while a backup could keep the
/// extraction going (`lending_possible`, see [`extraction_fed_by_duplicates`]). Without that
/// (`RawArticle`, `DupeArticleFallback=no` or `article`, or no `DirectUnpack`) no kind found later could
/// save anything, and waiting would only download the dead copy in full (B25, functional
/// `dupefailoverraw`; B48/B49 `projectedswap`, `projectededge`, `parlesscritical`).
///
/// Nor is it once TwinCheck labelled the backup (`labelled`: a `DupeKind` of `twin`, `alt`,
/// `container`, `other`, ...): that label is what [`alt_covers_lost_articles`] reads for a copy
/// of unknown kind. The kind itself is set as a copy is added (fleet intake, dupe search), so a
/// plain duplicate never gets one later: waiting on for it only downloaded a doomed copy further
/// (functional `pardamageborrow`, labelled `container` within 2 s, then held for its 120 s).
/// The fleet intake still reading a copy's video holds it all the same (`checking`).
fn backup_still_unclassified(
    kind: crate::donorkind::DonorKind,
    labelled: bool,
    parked_at: i64,
    now: i64,
    checking: bool,
    lending_possible: bool,
) -> bool {
    // the packing kind says nothing about the video: a copy still being read may carry another
    checking
        || (lending_possible
            && kind == crate::donorkind::DonorKind::Unknown
            && !labelled
            && now - parked_at < BACKUP_CLASSIFY_GRACE_SEC)
}

/// Whether TwinCheck labelled `backup_ptr` (see [`backup_still_unclassified`]).
#[cfg(unix)]
fn backup_twincheck_labelled(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    backup_ptr: NzbRef,
) -> bool {
    host.nzb_get_param(backup_ptr, b"DupeKind")
        .is_some_and(|k| !k.trim_ascii().is_empty())
}

/// Whether a duplicate's bytes can feed the extraction while the download runs (the
/// `lending_possible` argument of [`backup_still_unclassified`]): live borrowing (see
/// [`live_borrowing`]), or live stream repair (`DupeArticleFallback=stream`) patching what
/// direct unpack extracted, which keeps a download whose extraction a duplicate already
/// patched (`dupe_recovered_bytes`, see `check_dupe_failover`). Not with
/// `DupeArticleFallback=no` or `article`, without `DirectUnpack`, or with `RawArticle`.
#[cfg(unix)]
fn extraction_fed_by_duplicates(opts: &crate::queuesubsys_host::GlobalOptsSnap) -> bool {
    opts.dupe_article_fallback >= crate::queuesubsys_host::DAF_STREAM
        && opts.direct_unpack
        && !opts.raw_article
}

/// Whether the live dead-volume borrow can feed a running extraction (the `live_borrowing`
/// argument of [`alt_covers_lost_articles`]): `DupeArticleFallback=live` with `DirectUnpack`,
/// and a duplicate can lend at all: not with `RawArticle`, which keeps articles encoded
/// (article borrowing, stream repair and extraction all skip it).
#[cfg(unix)]
fn live_borrowing(opts: &crate::queuesubsys_host::GlobalOptsSnap) -> bool {
    opts.dupe_article_fallback == crate::queuesubsys_host::DAF_LIVE
        && opts.direct_unpack
        && !opts.raw_article
}
#[cfg(unix)]
const DUPE_PROBE_SAMPLE_COUNT: i32 = 10;
#[cfg(unix)]
const DUPE_PROBE_MIN_ARTICLES: usize = 4;
#[cfg(unix)]
const DUPE_PROBE_MIN_ALIVE_SAMPLES: i32 = 2;

#[cfg(unix)]
pub struct NzbgetRsQueueCoordinator {
    live_startups: crate::parchecker::LiveStartupFence,
    callback_metrics: Option<Box<crate::callback_metrics::Histogram>>,
    downloads_limit: std::sync::atomic::AtomicI32,
    server_config_generation: std::sync::atomic::AtomicI32,
    propagation_wait_logged_id: std::sync::atomic::AtomicI32,
    donor_cache: std::sync::Mutex<crate::dupefallback::DonorCacheState>,
    par_loaders_stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    par_loaders: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>,
    pub(crate) stopped: std::sync::atomic::AtomicBool,
    has_more_jobs: std::sync::atomic::AtomicBool,
    state_changed: std::sync::atomic::AtomicBool,
    mass_edit: std::sync::atomic::AtomicBool,
    want_save: std::sync::atomic::AtomicBool,
    history_changed: std::sync::atomic::AtomicBool,
    wait_lock: (std::sync::Mutex<bool>, std::sync::Condvar),
    active_downloads:
        std::sync::Mutex<Vec<std::sync::Arc<crate::articledl::NzbgetRsArticleDownloader>>>,
    active_dl_threads: (std::sync::Mutex<usize>, std::sync::Condvar),
}

#[cfg(unix)]
impl Drop for NzbgetRsQueueCoordinator {
    fn drop(&mut self) {
        self.par_loaders_stopped
            .store(true, std::sync::atomic::Ordering::Release);
        let handles: Vec<_> = self
            .par_loaders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        for h in handles {
            let _ = h.join();
        }
    }
}

#[cfg(unix)]
impl Default for NzbgetRsQueueCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(unix)]
fn automatic_live_par_repair(opts: &crate::queuesubsys_host::GlobalOptsSnap) -> bool {
    opts.par_check != crate::queuesubsys_host::PC_MANUAL
        && crate::options::active_options().is_some_and(|o| o.scalars().par_repair != 0)
}

/// The file a download's articles are written into while it downloads (its DirectWrite
/// output, `<dest>/<id>.out.tmp`), once created. Live PAR reads a slice back from it when
/// RAM no longer holds the slice's bytes. `None` without DirectWrite: each article then
/// waits in a file of its own until the download completes.
fn live_written_file(file: &crate::queuesubsys_host::FileSnap) -> Option<std::path::PathBuf> {
    (file.output_initialized && !file.output_filename.is_empty())
        .then(|| crate::fsops::path(&file.output_filename).to_path_buf())
}

/// Live par2 syndromes while downloading: automatic repair plus the
/// `LiveParRepair` switch. Without it no live session starts and repair runs
/// after the download; damage tracking and recovery-volume unpausing
/// (`pardamage`) still follow [`automatic_live_par_repair`] alone.
#[cfg(unix)]
fn live_par_syndromes(opts: &crate::queuesubsys_host::GlobalOptsSnap) -> bool {
    automatic_live_par_repair(opts) && crate::options::live_par_repair()
}

#[cfg(unix)]
pub(crate) fn live_start_context_matches(
    nzb: &crate::queuesubsys_host::NzbSnap,
    root: &std::path::Path,
    automatic: bool,
) -> bool {
    automatic
        && !nzb.deleting
        && !nzb.many_dupe_files
        && crate::fsops::path(&nzb.dest_dir) == root
        && (!nzb.has_post_info
            || !nzb.post_working && nzb.post_stage == PostStage::Queued)
}

#[cfg(unix)]
fn snapshot_file_articles(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file_ptr: FileRef,
    len: usize,
) -> Vec<crate::queuesubsys_host::ArticleSnap> {
    let mut v = Vec::with_capacity(len);
    for i in 0..len {
        v.push(host.article_at(file_ptr, i));
    }
    v
}

#[cfg(unix)]
fn pack_merge_split_counters(
    n: &crate::queuesubsys_host::NzbSnap,
) -> NzbgetRsNzbMergeSplitCounters {
    NzbgetRsNzbMergeSplitCounters {
        file_count: n.file_count,
        active_downloads: n.active_downloads,
        full_content_hash: n.full_content_hash,
        filtered_content_hash: n.filtered_content_hash,
        size: n.size,
        remaining_size: n.remaining_size,
        paused_file_count: n.paused_file_count,
        paused_size: n.paused_size,
        success_size: n.success_size,
        current_success_size: n.current_success_size,
        failed_size: n.failed_size,
        current_failed_size: n.current_failed_size,
        par_size: n.par_size,
        par_success_size: n.par_success_size,
        par_current_success_size: n.par_current_success_size,
        par_failed_size: n.par_failed_size,
        par_current_failed_size: n.par_current_failed_size,
        remaining_par_count: n.remaining_par_count,
        total_articles: n.total_articles,
        success_articles: n.success_articles,
        failed_articles: n.failed_articles,
        current_success_articles: n.current_success_articles,
        current_failed_articles: n.current_failed_articles,
        min_time: n.min_time,
        max_time: n.max_time,
        downloaded_size: n.downloaded_size,
        download_sec: n.download_sec,
        download_start_time: n.download_start_time,
    }
}

#[cfg(unix)]
fn join_path(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir.len() + 1 + name.len());
    out.extend_from_slice(dir);
    out.push(crate::paths::PATH_SEPARATOR);
    out.extend_from_slice(name);
    out
}

/// The IDs of the files of the download's video set: the files whose name has the same set stem
/// (the name without its volume number: `.partNN.rar`, `.rNN`, `.rar`, `.7z.NNN`, `.zNN`,
/// `.zip`, `.NNN`) as its largest file. A frontier cut covers only these: extras (subs, a
/// sample, a second set) are not lent by the alt. (Files already completed have left the
/// queue's list; the stem needs none of them.)
#[cfg(unix)]
fn video_set_file_ids(host: &crate::queuesubsys_host::QueueSubsysHost, nzb_ptr: NzbRef) -> Vec<i32> {
    let files: Vec<_> = (0..host.nzb_file_len(nzb_ptr))
        .map(|j| host.file_snap(host.nzb_file_at(nzb_ptr, j)))
        .filter(|f| !f.par_file)
        .collect();
    let Some(largest) = files.iter().max_by_key(|f| f.size) else {
        return Vec::new();
    };
    let stem = set_stem(&largest.filename);
    files.iter().filter(|f| set_stem(&f.filename) == stem).map(|f| f.id).collect()
}

/// A volume's name without its volume number, lower case (see [`video_set_file_ids`]).
fn set_stem(name: &[u8]) -> Vec<u8> {
    fn numeric(b: &[u8]) -> bool {
        !b.is_empty() && b.iter().all(u8::is_ascii_digit)
    }
    fn split_ext(n: &[u8]) -> Option<(&[u8], &[u8])> {
        let dot = n.iter().rposition(|&b| b == b'.')?;
        Some((&n[..dot], &n[dot + 1..]))
    }
    let n = name.to_ascii_lowercase();
    let Some((base, ext)) = split_ext(&n) else {
        return n;
    };
    if ext == b"rar" {
        // name.partNN.rar or name.rar
        if let Some((inner, part)) = split_ext(base) {
            if part.starts_with(b"part") && numeric(&part[4..]) {
                return inner.to_vec();
            }
        }
        return base.to_vec();
    }
    let numbered = ext.len() >= 2 && matches!(ext[0], b'r' | b's' | b'z') && numeric(&ext[1..]);
    if numbered || ext == b"zip" || numeric(ext) {
        return base.to_vec();
    }
    n
}


/// The share of a download's data (par2 left out) that came, success or
/// failure, in permille of its total size.
#[cfg(unix)]
fn data_done_permille(nzb: &crate::queuesubsys_host::NzbSnap) -> i32 {
    let done = (nzb.current_success_size + nzb.current_failed_size)
        - (nzb.par_current_success_size + nzb.par_current_failed_size);
    let total = nzb.size - nzb.par_size;
    if total <= 0 {
        return 0;
    }
    ((i128::from(done.max(0)) * 1000) / i128::from(total)).clamp(0, 1000) as i32
}

#[cfg(unix)]
fn backup_alive_permille(host: &crate::queuesubsys_host::QueueSubsysHost, nzb_ptr: NzbRef) -> i32 {
    let mut best = -1i32;
    for key in [b"DupeAlive".as_slice(), b"DupeHealth".as_slice()] {
        if let Some(val) = host.nzb_get_param(nzb_ptr, key) {
            best = best.max(crate::dupecoord::parse_alive_permille_bytes(&val));
        }
    }
    best
}

#[cfg(unix)]
impl NzbgetRsQueueCoordinator {
    pub fn new() -> Self {
        Self {
            live_startups: crate::parchecker::LiveStartupFence::default(),
            callback_metrics: crate::callback_metrics::Histogram::configured(),
            downloads_limit: std::sync::atomic::AtomicI32::new(0),
            server_config_generation: std::sync::atomic::AtomicI32::new(0),
            propagation_wait_logged_id: std::sync::atomic::AtomicI32::new(0),
            donor_cache: std::sync::Mutex::new(crate::dupefallback::DonorCacheState::default()),
            par_loaders_stopped: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            par_loaders: std::sync::Mutex::new(Vec::new()),
            stopped: std::sync::atomic::AtomicBool::new(false),
            has_more_jobs: std::sync::atomic::AtomicBool::new(true),
            state_changed: std::sync::atomic::AtomicBool::new(false),
            mass_edit: std::sync::atomic::AtomicBool::new(false),
            want_save: std::sync::atomic::AtomicBool::new(false),
            history_changed: std::sync::atomic::AtomicBool::new(false),
            wait_lock: (std::sync::Mutex::new(false), std::sync::Condvar::new()),
            active_downloads: std::sync::Mutex::new(Vec::new()),
            active_dl_threads: (std::sync::Mutex::new(0), std::sync::Condvar::new()),
        }
    }

    pub(crate) fn spawn_par_loader(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_id: i32,
        par_files: Vec<Vec<u8>>,
    ) {
        let mut guard = self.par_loaders.lock().unwrap_or_else(|e| e.into_inner());
        let mut alive = Vec::with_capacity(guard.len() + 1);
        for h in guard.drain(..) {
            if h.is_finished() {
                let _ = h.join();
            } else {
                alive.push(h);
            }
        }
        let ctx_addr = host.ctx as usize;
        let cb = host.cb;
        // keep a mock host a mock: only the real host answers ops in Rust
        let use_active = host.use_active;
        let stopped = std::sync::Arc::clone(&self.par_loaders_stopped);
        if let Ok(handle) = std::thread::Builder::new().spawn(move || {
            let loader_host = crate::queuesubsys_host::QueueSubsysHost {
                ctx: ctx_addr as *mut std::ffi::c_void,
                cb,
                use_active,
            };
            crate::directrenamer::run_par_loader(loader_host, &stopped, nzb_id, par_files);
        }) {
            alive.push(handle);
        }
        *guard = alive;
    }

    fn with_donor_cache<R>(
        &self,
        f: impl FnOnce(&mut crate::dupefallback::DonorCacheState) -> R,
    ) -> R {
        let mut guard = self.donor_cache.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    pub(crate) fn init(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        self.par_loaders_stopped
            .store(false, std::sync::atomic::Ordering::Release);
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Creating QueueCoordinator",
        );
        host.coord_dq_init();
        crate::dupeprobe::probe_reset();
        crate::twin::reset_all();
    }

    pub(crate) fn destroy(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Destroying QueueCoordinator",
        );
        self.par_loaders_stopped
            .store(true, std::sync::atomic::Ordering::Release);
        let loader_handles: Vec<_> = self
            .par_loaders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        for h in loader_handles {
            let _ = h.join();
        }
        crate::dupeprobe::probe_wait_all();
        crate::twin::wait_all();
        host.delete_all_active_downloads();
        if let Some(metrics) = &self.callback_metrics {
            metrics.emit_after_join();
        }
        // Live repair helpers were cancelled with their sessions at stop; give them a
        // bounded moment to leave the files before the final cleanup.
        crate::postproc::live_par_helpers_wait(std::time::Duration::from_secs(10));
        crate::parchecker::streaming_sessions_forget_all();
        host.coord_dq_final();
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"QueueCoordinator destroyed",
        );
    }

    fn stop_live_startups(&self) {
        self.live_startups.stop(|| {
            self.stopped
                .store(true, std::sync::atomic::Ordering::SeqCst);
            crate::parchecker::streaming_sessions_cancel_all()
        });
    }

    pub(crate) fn stop(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        self.stop_live_startups();
        self.par_loaders_stopped
            .store(true, std::sync::atomic::Ordering::Release);
        crate::dupeprobe::probe_stop_all();
        crate::twin::stop_all_jobs();
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Stopping ArticleDownloads",
        );
        host.with_locked_queue(|_| {
            let n = host.active_downloads_len();
            for i in 0..n {
                let ad = host.active_download_at(i);
                host.ad_action(ad.ad_ptr, crate::queuesubsys_host::AD_ACT_STOP, 0);
            }
        });
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"ArticleDownloads are notified",
        );
    }

    fn load(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.with_locked_queue(|dq_ptr| {
            let opts = host.opts();
            let mut stat_loaded = true;
            let mut perfect_server_match = true;
            let mut queue_loaded = false;

            if opts.server_mode {
                let (sl, pm) = host.stat_meter_load();
                stat_loaded = sl;
                perfect_server_match = pm;
                if host.disk_queue_exists() {
                    queue_loaded = host.disk_load_queue(dq_ptr);
                } else {
                    host.disk_discard_queue();
                }
            }

            if queue_loaded && stat_loaded {
                host.disk_cleanup_temp_dir(dq_ptr);
            }

            if queue_loaded && host.disk_legacy_queue_read() {
                host.log(
                    crate::queuesubsys_host::MK_INFO,
                    b"Saving the queue and history of an earlier build in the upstream format",
                );
                host.dq_history_changed(dq_ptr);
                host.dq_save(dq_ptr);
            }

            if queue_loaded && stat_loaded && !perfect_server_match {
                host.log(
                    crate::queuesubsys_host::MK_DEBUG,
                    b"Changes in section <NEWS SERVERS> of config file detected, resaving queue",
                );
                host.stat_meter_save();
                host.dq_history_changed(dq_ptr);
                host.dq_save(dq_ptr);

                if opts.server_mode {
                    let qlen = host.queue_len(dq_ptr);
                    for i in 0..qlen {
                        let nzb_ptr = host.queue_at(dq_ptr, i);
                        if opts.continue_partial {
                            let flen = host.nzb_file_len(nzb_ptr);
                            for j in 0..flen {
                                let fp = host.nzb_file_at(nzb_ptr, j);
                                let f = host.file_snap(fp);
                                if f.articles_len > 0 {
                                    host.disk_save_file_state(fp, false);
                                }
                            }
                        }
                        for cf in host.nzb_completed_files(nzb_ptr) {
                            if (cf.status == crate::queuesubsys_host::CF_PARTIAL
                                || cf.status == crate::queuesubsys_host::CF_FAILURE)
                                && cf.id > 0
                            {
                                host.disk_resave_completed_file_state(cf.id);
                            }
                        }
                    }
                }
            }

            host.coord_dq_loaded();
        });
    }

    fn adjust_downloads_limit(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        let cur_gen = host.server_pool_generation();
        if self
            .server_config_generation
            .load(std::sync::atomic::Ordering::Relaxed)
            == cur_gen
        {
            return;
        }
        let (levels, actives, conns) = host.server_pool_servers();
        let limit = calc_downloads_limit(&levels, &actives, &conns);
        self.downloads_limit
            .store(limit, std::sync::atomic::Ordering::Relaxed);
        self.server_config_generation
            .store(cur_gen, std::sync::atomic::Ordering::Relaxed);
    }

    fn wait_jobs(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"QueueCoordinator: waiting for Downloads to complete",
        );
        loop {
            let mut empty = false;
            host.with_locked_queue(|_| {
                empty = host.active_downloads_len() == 0;
            });
            if empty {
                break;
            }
            host.sleep_ms(100);
            self.reset_hanging_downloads(host);
        }
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"QueueCoordinator: Downloads are completed",
        );
    }

    pub(crate) fn run(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Entering QueueCoordinator-loop",
        );
        self.load(host);
        self.adjust_downloads_limit(host);
        let mut was_standby = true;
        let mut article_downloads_running = false;
        let mut last_reset: i64 = 0;
        host.stat_meter_interval_check();
        let mut wait_interval: i32 = 100;

        while !host.self_is_stopped() {
            let mut downloads_checked = false;
            let mut download_started = false;

            host.with_locked_queue(|dq_ptr| {
                let opts = host.opts();
                let next_pair = self.get_next_article(host, &opts, dq_ptr);
                // The download next in line waits while its dead-pick probe waits for a
                // connection (see `dupeprobe::probe_waiting_for_connection`): the connection it
                // would take goes to the probe. Nothing of lower priority takes it meanwhile
                // (a fleet's primary stays first), and the queue stays busy (`PROBE_HELD`).
                let held = next_pair.as_ref().is_some_and(|(file, _)| {
                    crate::dupeprobe::probe_waiting_for_connection(
                        host.nzb_snap(file.nzb_ptr, false).id,
                    )
                });
                PROBE_HELD.store(held, std::sync::atomic::Ordering::Relaxed);
                let next_pair = if held { None } else { next_pair };
                let has_more_articles = next_pair.is_some() || held;
                let active_len = host.active_downloads_len();
                article_downloads_running = active_len > 0;
                host.self_set_has_more_jobs(has_more_articles || article_downloads_running);

                let dl_limit = self
                    .downloads_limit
                    .load(std::sync::atomic::Ordering::Relaxed);
                let effective_limit = host.effective_downloads_limit(dl_limit);
                let can_proceed = !host.self_is_stopped() && (active_len as i32) < effective_limit;

                if let Some((file, art)) = next_pair {
                    if can_proceed && (!opts.download_held || file.extra_priority) {
                        let nzb = host.nzb_snap(file.nzb_ptr, false);
                        let conn_ptr = host.server_pool_get_connection(
                            nzb.has_desired_server,
                            nzb.desired_server_id,
                        );
                        if conn_ptr != 0 {
                            downloads_checked = true;
                            article_downloads_running = true;
                            download_started = true;
                            self.start_dead_pick_probe(host, &opts, dq_ptr, &nzb);
                            self.start_article_download(host, &opts, &nzb, &file, &art, conn_ptr);
                        } else if nzb.has_desired_server {
                            let mut dbg = b"Could not start download for ".to_vec();
                            dbg.extend_from_slice(&nzb.name);
                            dbg.extend_from_slice(
                                format!(
                                    ": desired server {} has no available connections",
                                    nzb.desired_server_id
                                )
                                .as_bytes(),
                            );
                            host.log(crate::queuesubsys_host::MK_DEBUG, &dbg);
                        }
                    }
                }
            });

            crate::parchecker::streaming_session_retry_completion_tick();
            self.drain_live_par_completed_files(host);
            Self::drain_live_par_articles(host);
            self.order_by_names(host);
            self.drain_live_heads(host);
            self.restart_live_stragglers(host);

            if !downloads_checked {
                host.with_locked_queue(|_| {
                    article_downloads_running = host.active_downloads_len() > 0;
                });
            }

            // (a download held for its probe's connection resumes as soon as the probe has one)
            let standby =
                !article_downloads_running && !PROBE_HELD.load(std::sync::atomic::Ordering::Relaxed);
            if standby != was_standby {
                host.stat_meter_enter_leave_standby(standby);
                host.work_state_set_downloading(!standby);
                was_standby = standby;
                if standby {
                    self.save_all_partial_state(host);
                }
            }

            if standby {
                wait_interval = (wait_interval * 2).min(2000);
                host.self_standby_wait(wait_interval);
            } else {
                let sleep_interval = if download_started { 0 } else { 5 };
                host.sleep_ms(sleep_interval);
                host.stat_meter_add_zero_speed();
                wait_interval = 100;
            }

            let now = host.opts().cur_time;
            if last_reset != now {
                // F16a: the fleets' failover order, as their members show it
                host.with_locked_queue(|dq_ptr| crate::fleethold::refresh_all_host(host, dq_ptr));
                host.server_pool_close_unused();
                self.reset_hanging_downloads(host);
                if !standby {
                    self.save_all_partial_state(host);
                }
                host.stat_meter_interval_check();
                host.log_interval_check();
                self.adjust_downloads_limit(host);
                host.set_standby_mode(standby);
                last_reset = host.opts().cur_time;
            }
        }

        self.wait_jobs(host);
        self.save_all_partial_state(host);
        self.save_queue_if_changed(host);
        self.save_all_file_state(host);

        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Exiting QueueCoordinator-loop",
        );
    }

    pub(crate) fn add_nzb_file_to_queue(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        uptr_addr: usize,
        url_ptr: NzbRef,
        add_first: bool,
    ) -> NzbRef {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Adding NZBFile to queue",
        );
        let mut out_ptr = NzbRef::NONE;
        host.with_locked_queue(|dq_ptr| {
            let opts = host.opts();
            // Read raw NzbInfo* from std::unique_ptr<NzbInfo>* at uptr_addr
            let raw_nzb_ptr = unsafe { *(uptr_addr as *const usize) };
            let mut added_nzb_ptr = NzbRef::from_addr(raw_nzb_ptr);

            host.dq_notify(
                dq_ptr,
                crate::queuesubsys_host::ASPECT_NZB_FOUND,
                added_nzb_ptr,
                FileRef::NONE,
            );

            let nzb = host.nzb_snap(added_nzb_ptr, false);
            let delete_status = nzb.delete_status;

            if delete_status != DeleteStatus::None {
                let flen = host.nzb_file_len(added_nzb_ptr);
                let mut all_paused = flen > 0;
                for i in 0..flen {
                    let fp = host.nzb_file_at(added_nzb_ptr, i);
                    let f = host.file_snap(fp);
                    all_paused &= f.paused;
                    if opts.server_mode {
                        host.disk_discard_file(f.id, true, false, false);
                    }
                }
                host.nzb_set_int(
                    added_nzb_ptr,
                    crate::queuesubsys_host::NZB_INT_DELETE_PAUSED,
                    i32::from(all_paused),
                );
            }

            if delete_status == DeleteStatus::None {
                if opts.dupe_check && nzb.dupe_mode != DupeMode::Force {
                    self.check_dupe_file_infos(host, added_nzb_ptr);
                }
                host.queue_insert_owned(dq_ptr, uptr_addr, url_ptr, add_first);
            } else {
                host.queue_insert_owned(dq_ptr, uptr_addr, NzbRef::NONE, true);
            }

            if url_ptr.is_some() {
                let url_snap = host.nzb_snap(url_ptr, false);
                host.nzb_set_int(
                    added_nzb_ptr,
                    crate::queuesubsys_host::NZB_INT_ID,
                    url_snap.id,
                );
                host.queue_remove(dq_ptr, url_ptr);
            }

            let cur_nzb = host.nzb_snap(added_nzb_ptr, false);
            if delete_status == DeleteStatus::None {
                let mut msg = b"Collection ".to_vec();
                msg.extend_from_slice(&cur_nzb.name);
                msg.extend_from_slice(b" added to queue");
                host.nzb_print_message(added_nzb_ptr, crate::queuesubsys_host::MK_INFO, &msg);
            }

            if delete_status != DeleteStatus::Manual {
                host.dq_notify(
                    dq_ptr,
                    crate::queuesubsys_host::ASPECT_NZB_ADDED,
                    added_nzb_ptr,
                    FileRef::NONE,
                );
            }

            if delete_status != DeleteStatus::None {
                let check_id = cur_nzb.id;
                host.queue_remove(dq_ptr, added_nzb_ptr);
                if host.history_find_by_id(dq_ptr, check_id) == 0 {
                    added_nzb_ptr = NzbRef::NONE;
                }
            }

            host.dq_save(dq_ptr);
            out_ptr = added_nzb_ptr;
        });
        out_ptr
    }

    pub(crate) fn check_dupe_file_infos(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_ptr: NzbRef,
    ) {
        host.log(crate::queuesubsys_host::MK_DEBUG, b"CheckDupeFileInfos");
        if nzb_ptr.is_none() {
            return;
        }
        let flen = host.nzb_file_len(nzb_ptr);
        if flen == 0 {
            return;
        }
        let opts = host.opts();
        let mut snaps = Vec::with_capacity(flen);
        let mut sizes = Vec::with_capacity(flen);
        for i in 0..flen {
            let f = host.file_snap(host.nzb_file_at(nzb_ptr, i));
            sizes.push(f.size);
            snaps.push(f);
        }
        let names: Vec<&[u8]> = snaps.iter().map(|f| f.filename.as_slice()).collect();
        let mut dupes = vec![0usize; flen];
        let count = find_dupe_file_indices(&names, &sizes, &mut dupes);
        for &idx in &dupes[..count] {
            let f = &snaps[idx];
            let mut msg = b"File \"".to_vec();
            msg.extend_from_slice(&f.filename);
            msg.extend_from_slice(b"\" appears twice in collection, adding only the biggest file");
            host.log(crate::queuesubsys_host::MK_WARNING, &msg);
            host.nzb_update_deleted_stats(nzb_ptr, f.ptr);
            host.nzb_remove_file(nzb_ptr, f.ptr);
            if opts.server_mode {
                host.disk_discard_file(f.id, true, false, false);
            }
        }
    }

    fn discard_temp_files(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        file: &crate::queuesubsys_host::FileSnap,
    ) {
        discard_temp_files_host(host, opts, file);
    }

    pub(crate) fn fail_over_dead_pick(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_id: i32,
        samples: i32,
        missing_servers: i32,
        active_servers: i32,
        recovered_at_start: i32,
    ) {
        let nzb_ptr = host.queue_find_by_id(dq_ptr, nzb_id);
        if nzb_ptr.is_none() {
            return;
        }
        let nzb = host.nzb_snap(nzb_ptr, false);
        if nzb.deleting || nzb.parking || nzb.delete_status != DeleteStatus::None {
            return;
        }
        let own_success = nzb.current_success_articles
            - (nzb.dupe_recovered_articles - recovered_at_start).max(0);
        if own_success >= DUPE_PROBE_MIN_ALIVE_SAMPLES {
            return;
        }
        let opts = host.opts();
        let backup = if crate::dupecoord::fails_over_snap(&opts, &nzb) {
            crate::dupecoord::find_dupe_backup_host(
                host,
                &opts,
                dq_ptr,
                nzb.ptr,
                &nzb.name,
                &nzb.dupe_key,
            )
            .filter(|(_, b)| !crate::dupecoord::locked_host(host, b.ptr))
        } else {
            None
        };
        let Some((_, backup_nzb)) = backup else {
            return;
        };
        if backup_continues_extraction(host, &opts, &nzb, &backup_nzb)
            || !crate::dupecoord::dupe_failover_warranted_inline(
                nzb.dupe_score,
                0,
                backup_nzb.dupe_score,
            )
        {
            return;
        }
        note_cannot_continue(host, &opts, &nzb, &backup_nzb);
        let mut msg = b"Failing over ".to_vec();
        msg.extend_from_slice(&nzb.name);
        msg.extend_from_slice(b" to duplicate ");
        msg.extend_from_slice(&backup_nzb.name);
        msg.extend_from_slice(
            format!(
                ": none of {} sampled articles exists on any server ({} of {} servers answered definitively)",
                samples, missing_servers, active_servers
            )
            .as_bytes(),
        );
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
        // judged dead from the sample, whatever its Health says
        host.nzb_set_param(nzb.ptr, crate::dupecoord::PROJECTED_PARAM, Some(b"0.0"));
        crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
    }

    pub(crate) fn delete_queue_entry(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        file_ptr: FileRef,
    ) -> bool {
        host.file_set_int(file_ptr, crate::queuesubsys_host::FILE_INT_DELETED, 1);
        let mut downloading = false;
        let n = host.active_downloads_len();
        for i in 0..n {
            let ad = host.active_download_at(i);
            if ad.file_ptr == file_ptr {
                downloading = true;
                host.ad_action(ad.ad_ptr, crate::queuesubsys_host::AD_ACT_STOP, 0);
            }
        }
        if !downloading {
            self.delete_file_info(host, dq_ptr, file_ptr, false);
        }
        downloading
    }

    pub(crate) fn set_queue_entry_category(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_ptr: NzbRef,
        category: &[u8],
    ) -> bool {
        let nzb = host.nzb_snap(nzb_ptr, false);
        if nzb.has_post_info {
            let mut msg = b"Could not change category for ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b". File in post-process-stage");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }
        let old_dest_dir = nzb.dest_dir;
        host.nzb_set_str(nzb_ptr, crate::queuesubsys_host::NZB_STR_CATEGORY, category);
        host.nzb_action(nzb_ptr, crate::queuesubsys_host::NZB_ACT_BUILD_DEST_DIR);
        let new_nzb = host.nzb_snap(nzb_ptr, false);
        new_nzb.dest_dir == old_dest_dir || host.move_completed_files(nzb_ptr, &old_dest_dir)
    }

    pub(crate) fn set_queue_entry_name(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_ptr: NzbRef,
        name: &[u8],
    ) -> bool {
        let nzb = host.nzb_snap(nzb_ptr, false);
        if nzb.has_post_info {
            let mut msg = b"Could not rename ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b". File in post-process-stage");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }
        if name.is_empty() {
            let mut msg = b"Could not rename ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b". The new name cannot be empty");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }
        let nice = crate::downloadinfo::make_nice_nzb_name(name, false);
        host.nzb_set_str(nzb_ptr, crate::queuesubsys_host::NZB_STR_NAME, &nice);

        if nzb.kind == crate::queuesubsys_host::NK_URL {
            let mut fn_buf = nice;
            fn_buf.extend_from_slice(b".nzb");
            host.nzb_set_str(nzb_ptr, crate::queuesubsys_host::NZB_STR_FILENAME, &fn_buf);
            return true;
        }

        let old_dest_dir = nzb.dest_dir;
        host.nzb_action(nzb_ptr, crate::queuesubsys_host::NZB_ACT_BUILD_DEST_DIR);
        let new_nzb = host.nzb_snap(nzb_ptr, false);
        new_nzb.dest_dir == old_dest_dir || host.move_completed_files(nzb_ptr, &old_dest_dir)
    }

    pub(crate) fn merge_queue_entries(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        dest_ptr: NzbRef,
        src_ptr: NzbRef,
    ) -> bool {
        let dest = host.nzb_snap(dest_ptr, false);
        let src = host.nzb_snap(src_ptr, false);
        if dest.has_post_info || src.has_post_info {
            let mut msg = b"Could not merge ".to_vec();
            msg.extend_from_slice(&dest.name);
            msg.extend_from_slice(b" and ");
            msg.extend_from_slice(&src.name);
            msg.extend_from_slice(b". File in post-process-stage");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }
        if dest.kind == crate::queuesubsys_host::NK_URL
            || src.kind == crate::queuesubsys_host::NK_URL
        {
            let mut msg = b"Could not merge ".to_vec();
            msg.extend_from_slice(&dest.name);
            msg.extend_from_slice(b" and ");
            msg.extend_from_slice(&src.name);
            msg.extend_from_slice(b". URLs cannot be merged");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }

        crate::postproc::direct_unpack_join_live_par_repair(dest.id);
        crate::postproc::direct_unpack_join_live_par_repair(src.id);
        let old_dest_dir = src.dest_dir.clone();
        host.nzb_set_str(
            src_ptr,
            crate::queuesubsys_host::NZB_STR_CATEGORY,
            &dest.category,
        );
        host.nzb_set_str(
            src_ptr,
            crate::queuesubsys_host::NZB_STR_DEST_DIR,
            &dest.dest_dir,
        );
        if old_dest_dir != dest.dest_dir {
            let _ = host.move_completed_files(src_ptr, &old_dest_dir);
        }

        let mut dc = pack_merge_split_counters(&dest);
        let sc = pack_merge_split_counters(&src);
        merge_counters(&mut dc, &sc);

        host.nzb_merge_lists_and_stats(dest_ptr, src_ptr);
        host.nzb_apply_merge_split_counters(dest_ptr, &dc);

        let mut merged_qf = dest.queued_filename;
        merged_qf.push(b'|');
        merged_qf.extend_from_slice(&src.queued_filename);
        host.nzb_set_str(
            dest_ptr,
            crate::queuesubsys_host::NZB_STR_QUEUED_FILENAME,
            &merged_qf,
        );

        for param in [
            b"DupeFiles".as_slice(),
            b"DupeKind".as_slice(),
            crate::donorkind::KIND_PARAM,
            b"DupeSampled".as_slice(),
            b"DupeSampledOf".as_slice(),
            crate::dupeintake::INNER_PARAM,
            crate::twinfields::INNER_KEY_PARAM,
        ] {
            host.nzb_set_param(dest_ptr, param, None);
        }

        merge_dupe_par_params(host, dest_ptr, src_ptr);
        // what the source borrowed moves with its files, like its par2 counts
        host.nzb_set_int(
            dest_ptr,
            crate::queuesubsys_host::NZB_INT_DUPE_RECOVERED_ARTICLES,
            dest.dupe_recovered_articles
                .saturating_add(src.dupe_recovered_articles),
        );
        host.nzb_set_int(
            dest_ptr,
            crate::queuesubsys_host::NZB_INT_DUPE_RECOVERED_HOLES,
            dest.dupe_recovered_holes
                .saturating_add(src.dupe_recovered_holes),
        );
        host.nzb_set_i64(
            dest_ptr,
            crate::queuesubsys_host::NZB_I64_DUPE_RECOVERED_BYTES,
            dest.dupe_recovered_bytes
                .saturating_add(src.dupe_recovered_bytes),
        );

        host.disk_discard_files(src_ptr);
        crate::dupefallback::par_damage_forget_all(src.id);
        crate::postproc::live_par_time_discard(src.id);
        host.queue_remove(dq_ptr, src_ptr);
        true
    }

    pub(crate) fn split_queue_entries(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        file_ptrs: &[FileRef],
        name: &[u8],
    ) -> bool {
        let mut src_nzb_ptr = NzbRef::NONE;
        let mut file_snaps = Vec::with_capacity(file_ptrs.len());

        for &fp in file_ptrs {
            let f = host.file_snap(fp);
            let f_nzb = host.nzb_snap(f.nzb_ptr, false);
            let same_src = src_nzb_ptr.is_none() || f.nzb_ptr == src_nzb_ptr;
            let v = validate_split_file_state(
                f.active_downloads,
                f.completed_articles,
                f_nzb.has_post_info,
                same_src,
            );
            if v != 0 {
                let mut msg = b"Could not split ".to_vec();
                msg.extend_from_slice(&f.filename);
                msg.extend_from_slice(if v == 1 {
                    b". File is already (partially) downloaded"
                } else if v == 2 {
                    b". File in post-process-stage"
                } else {
                    b". The files belong to different collections"
                });
                host.log(crate::queuesubsys_host::MK_ERROR, &msg);
                return false;
            }
            src_nzb_ptr = f.nzb_ptr;
            file_snaps.push(f);
        }

        if src_nzb_ptr.is_none() {
            return false;
        }

        let src_snap = host.nzb_snap(src_nzb_ptr, false);
        crate::postproc::direct_unpack_join_live_par_repair(src_snap.id);
        let (uptr_addr, new_nzb_ptr) = host.alloc_new_nzb_uptr();

        host.nzb_set_str(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_STR_FILENAME,
            &src_snap.filename,
        );
        host.nzb_set_str(new_nzb_ptr, crate::queuesubsys_host::NZB_STR_NAME, name);
        host.nzb_set_str(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_STR_CATEGORY,
            &src_snap.category,
        );
        host.nzb_set_int(new_nzb_ptr, crate::queuesubsys_host::NZB_INT_FULL_HASH, 0);
        host.nzb_set_int(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_INT_FILTERED_HASH,
            0,
        );
        host.nzb_set_int(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_INT_PRIORITY,
            src_snap.priority,
        );
        host.nzb_action(new_nzb_ptr, crate::queuesubsys_host::NZB_ACT_BUILD_DEST_DIR);
        host.nzb_set_str(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_STR_QUEUED_FILENAME,
            &src_snap.queued_filename,
        );
        host.nzb_copy_params(new_nzb_ptr, src_nzb_ptr);
        // borrowed par2 counts stay with the source, as its recovered-article count does
        host.nzb_set_param(new_nzb_ptr, PAR_ARTICLES_PARAM, None);
        host.nzb_set_param(new_nzb_ptr, PAR_FILES_PARAM, None);
        // a split-off part is not another post of the fleet: drop its fleet
        // identity, as merge does on its destination
        for param in SPLIT_DROPPED_PARAMS {
            host.nzb_set_param(new_nzb_ptr, param, None);
        }

        let new_init_snap = host.nzb_snap(new_nzb_ptr, false);
        let mut sc = pack_merge_split_counters(&src_snap);
        let mut nc = pack_merge_split_counters(&new_init_snap);
        sc.full_content_hash = 0;
        sc.filtered_content_hash = 0;

        for f in &file_snaps {
            host.dq_notify(
                dq_ptr,
                crate::queuesubsys_host::ASPECT_FILE_DELETED,
                src_nzb_ptr,
                f.ptr,
            );
            host.nzb_transfer_split_file(src_nzb_ptr, new_nzb_ptr, f.ptr);
            let fc = NzbgetRsSplitFileCounters {
                size: f.size,
                remaining_size: f.remaining_size,
                success_size: f.success_size,
                failed_size: f.failed_size,
                missed_size: f.missed_size,
                total_articles: f.total_articles,
                missed_articles: f.missed_articles,
                success_articles: f.success_articles,
                failed_articles: f.failed_articles,
                par_file: i32::from(f.par_file),
                paused: i32::from(f.paused),
            };
            split_transfer_file(&mut sc, &mut nc, &fc);
        }

        host.nzb_apply_merge_split_counters(src_nzb_ptr, &sc);
        host.nzb_apply_merge_split_counters(new_nzb_ptr, &nc);

        host.nzb_action(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_ACT_UPDATE_MIN_MAX_TIME,
        );
        if host.nzb_completed_files(src_nzb_ptr).is_empty() {
            host.nzb_action(
                src_nzb_ptr,
                crate::queuesubsys_host::NZB_ACT_UPDATE_MIN_MAX_TIME,
            );
        }

        if host.nzb_file_len(src_nzb_ptr) == 0 {
            host.disk_discard_files(src_nzb_ptr);
            crate::dupefallback::par_damage_forget_all(src_snap.id);
            crate::postproc::live_par_time_discard(src_snap.id);
            host.queue_remove(dq_ptr, src_nzb_ptr);
        }

        host.finish_new_nzb_uptr(dq_ptr, uptr_addr);
        true
    }

    pub(crate) fn log_debug_info(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.with_locked_queue(|dq_ptr| {
            let opts = host.opts();
            host.log(crate::queuesubsys_host::MK_INFO, b"   ---------- Queue");
            let (remaining, remaining_forced) = host.dq_calc_remaining_size(dq_ptr);
            let rem_mb = (remaining as f64) / 1024.0 / 1024.0;
            let forced_mb = (remaining_forced as f64) / 1024.0 / 1024.0;
            host.log(
                crate::queuesubsys_host::MK_INFO,
                format!(
                    "     Remaining: {:.1} MB, Forced: {:.1} MB",
                    rem_mb, forced_mb
                )
                .as_bytes(),
            );
            let dl_state = if opts.pause_download {
                "paused"
            } else if opts.temp_pause_download {
                "temp-paused"
            } else {
                "active"
            };
            let pp_state = if opts.pause_post_process {
                "paused"
            } else {
                "active"
            };
            let sc_state = if opts.pause_scan { "paused" } else { "active" };
            host.log(
                crate::queuesubsys_host::MK_INFO,
                format!(
                    "     Download: {}, Post-process: {}, Scan: {}",
                    dl_state, pp_state, sc_state
                )
                .as_bytes(),
            );
            host.log(
                crate::queuesubsys_host::MK_INFO,
                b"   ---------- QueueCoordinator",
            );
            let active_n = host.active_downloads_len();
            let dl_limit = self
                .downloads_limit
                .load(std::sync::atomic::Ordering::Relaxed);
            host.log(
                crate::queuesubsys_host::MK_INFO,
                format!("    Active Downloads: {}, Limit: {}", active_n, dl_limit).as_bytes(),
            );
            for i in 0..active_n {
                let ad = host.active_download_at(i);
                host.ad_action(ad.ad_ptr, crate::queuesubsys_host::AD_ACT_LOG_DEBUG, 0);
            }
        });
    }

    fn get_next_article(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
    ) -> Option<(
        crate::queuesubsys_host::FileSnap,
        crate::queuesubsys_host::ArticleSnap,
    )> {
        let mut checked_files: Vec<FileRef> = Vec::new();
        let cur_date = opts.cur_time;
        // Downloads extracted live, by id: their volumes' extraction order and the lowest
        // unfinished volume (see `liveorder`), looked up once per call.
        let mut orders: Vec<(i32, Option<LiveOrder>)> = Vec::new();

        loop {
            let mut best_file: Option<(
                crate::queuesubsys_host::NzbSnap,
                crate::queuesubsys_host::FileSnap,
                Option<usize>,
            )> = None;
            let mut prop_wait_file: Option<(
                crate::queuesubsys_host::NzbSnap,
                crate::queuesubsys_host::FileSnap,
            )> = None;

            let qlen = host.queue_len(dq_ptr);
            for i in 0..qlen {
                let nzb_ptr = host.queue_at(dq_ptr, i);
                let nzb = host.nzb_snap(nzb_ptr, false);
                let flen = host.nzb_file_len(nzb_ptr);

                let nzb_higher = match &best_file {
                    None => true,
                    Some((best_nzb, _, _)) => {
                        if nzb.has_extra_priority == best_nzb.has_extra_priority {
                            nzb.priority > best_nzb.priority
                        } else {
                            nzb.has_extra_priority && !best_nzb.has_extra_priority
                        }
                    }
                };

                let nzb_paused = (flen as i32) == nzb.paused_file_count;

                if nzb_higher
                    && !nzb_paused
                    && !self.has_active_dupe(host, dq_ptr, &nzb)
                    && ((!opts.pause_download && !opts.quota_reached) || nzb.force_priority)
                {
                    let propagation_delay = opts.propagation_delay;
                    let order = cached_live_order(&mut orders, host, nzb_ptr, nzb.id);
                    // A par2 file with articles to fetch seen earlier in the list: the window
                    // does not go ahead of it (live PAR reads it early).
                    let mut par_before = false;
                    for j in 0..flen {
                        let fp = host.nzb_file_at(nzb_ptr, j);
                        if checked_files.contains(&fp) {
                            continue;
                        }
                        let f = host.file_snap(fp);
                        if f.paused || f.deleted {
                            continue;
                        }
                        let par_file = f.par_file;
                        let prop_wait = is_propagation_wait(f.time, propagation_delay, cur_date);
                        if !prop_wait {
                            // A volume of a set extracted live, within the window: taken in
                            // extraction order before the download's other files (a par2
                            // file ahead of it keeps its place: live PAR reads it early).
                            let rank = order.as_ref().and_then(|o| o.window_rank(&f.filename));
                            let higher = match &best_file {
                                None => true,
                                Some((best_nzb, bf, best_rank)) => {
                                    (f.extra_priority == bf.extra_priority
                                        && nzb.priority > best_nzb.priority)
                                        || (f.extra_priority && !bf.extra_priority)
                                        || (f.extra_priority == bf.extra_priority
                                            && best_nzb.ptr == nzb.ptr
                                            && !par_before
                                            && rank.is_some_and(|r| best_rank.is_none_or(|b| r < b)))
                                }
                            };
                            if higher {
                                best_file = Some((nzb.clone(), f, rank));
                            }
                            par_before |= par_file;
                        } else if prop_wait_file
                            .as_ref()
                            .is_none_or(|(_, pw)| f.time < pw.time)
                        {
                            prop_wait_file = Some((nzb.clone(), f));
                        }
                    }
                }
            }

            let Some((mut nzb, mut file, _)) = best_file else {
                if let Some((pw_nzb, pw_file)) = prop_wait_file {
                    let prev_logged = self
                        .propagation_wait_logged_id
                        .load(std::sync::atomic::Ordering::Relaxed);
                    if pw_file.id != prev_logged {
                        self.propagation_wait_logged_id
                            .store(pw_file.id, std::sync::atomic::Ordering::Relaxed);
                        let (posted_min, start_min) = calc_propagation_wait_log_mins(
                            pw_file.time,
                            opts.propagation_delay,
                            cur_date,
                        );
                        let mut msg = b"Holding ".to_vec();
                        msg.extend_from_slice(&pw_nzb.name);
                        msg.push(crate::paths::PATH_SEPARATOR);
                        msg.extend_from_slice(&pw_file.filename);
                        msg.extend_from_slice(
                            format!(
                                " due to PropagationDelay: posted {} min ago, download starts in {} min",
                                posted_min, start_min
                            )
                            .as_bytes(),
                        );
                        host.nzb_print_message(
                            pw_nzb.ptr,
                            crate::queuesubsys_host::MK_DETAIL,
                            &msg,
                        );
                    }
                }
                break;
            };

            if opts.direct_rename
                && nzb.direct_rename_status <= DirectRenameStatus::Running
                && !nzb.all_first
            {
                if let Some((f1, a1)) = self.get_next_first_article(host, opts, &nzb) {
                    self.propagation_wait_logged_id
                        .store(0, std::sync::atomic::Ordering::Relaxed);
                    return Some((f1, a1));
                }
            }

            // Headers a live extraction waits for come before anything else.
            if let Some((f1, a1)) = self.get_next_urgent_article(host, opts, &nzb) {
                self.propagation_wait_logged_id
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                return Some((f1, a1));
            }

            // `file` was read before the direct-rename pass, which may have
            // loaded this file's articles meanwhile: decide on a fresh read.
            file = self.ensure_articles_loaded(host, opts, &nzb, file.ptr);

            if let Some((idx, mut art)) = host.file_first_undefined_article(file.ptr) {
                if file.dupe_cutover && art.dupe_fallback_round == 0 {
                    art.dupe_proactive = true;
                    host.article_edit(art.ptr, crate::qarticle::ArticleEdit::Proactive(true));
                    let ok = self.with_donor_cache(|cache| {
                        crate::dupefallback::try_fallback_host(
                            cache, host, opts, dq_ptr, &mut nzb, &mut file, idx, &mut art,
                        )
                    });
                    art.dupe_proactive = ok;
                    host.article_edit(art.ptr, crate::qarticle::ArticleEdit::Proactive(ok));
                }
                self.propagation_wait_logged_id
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                return Some((file, art));
            }

            if checked_files.capacity() < 100 {
                checked_files.reserve(100);
            }
            checked_files.push(file.ptr);
        }

        None
    }

    /// An article a live extraction waits for before it can go on (see `liveorder::urge`): the
    /// one holding a header byte it asked for, or the next one of a volume it needs whole.
    fn get_next_urgent_article(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> Option<(
        crate::queuesubsys_host::FileSnap,
        crate::queuesubsys_host::ArticleSnap,
    )> {
        use crate::liveorder::Urgent;
        let urgent = crate::liveorder::urgent(nzb.id);
        if urgent.is_empty() {
            return None;
        }
        let flen = host.nzb_file_len(nzb.ptr);
        for u in urgent {
            let found = (0..flen).map(|i| host.nzb_file_at(nzb.ptr, i)).find(|&fp| {
                let f = host.file_snap(fp);
                !f.deleted && !f.paused && f.filename.eq_ignore_ascii_case(u.name())
            });
            let Some(fp) = found else {
                // Finished (or not in the queue): nothing to hand out.
                crate::liveorder::done_urgent(nzb.id, &u);
                continue;
            };
            let f = self.ensure_articles_loaded(host, opts, nzb, fp);
            let n = f.articles_len;
            // (a file led by duplicates gets its articles from them, in the usual order)
            if n == 0 || f.dupe_cutover {
                continue;
            }
            let candidates: Vec<usize> = match &u {
                Urgent::Whole { .. } => match host.file_first_undefined_article(fp) {
                    Some((_, art)) => return Some((f, art)),
                    None => Vec::new(),
                },
                Urgent::At { offset, .. } => {
                    let known: Vec<Option<(u64, u64)>> = (0..n)
                        .map(|i| {
                            let a = host.article_at(fp, i);
                            (a.segment_offset >= 0 && a.segment_size > 0)
                                .then(|| (a.segment_offset as u64, a.segment_size as u64))
                        })
                        .collect();
                    let total = if f.decoded_file_size > 0 {
                        f.decoded_file_size
                    } else {
                        f.size
                    }
                    .max(0) as u64;
                    let idx = crate::liveorder::article_for(*offset, &known, n, total);
                    let placed = known[idx].is_some_and(|(o, s)| o <= *offset && *offset < o + s);
                    if placed {
                        vec![idx]
                    } else {
                        // Placed by estimate: its neighbours too.
                        let mut c = vec![idx, (idx + 1).min(n - 1), idx.saturating_sub(1)];
                        c.dedup();
                        c
                    }
                }
            };
            for i in candidates {
                let a = host.article_at(fp, i);
                if a.status == crate::queuesubsys_host::AI_UNDEFINED {
                    return Some((f, a));
                }
            }
            // Handed out already: asked again if the extraction still waits once it is written.
            crate::liveorder::done_urgent(nzb.id, &u);
        }
        None
    }

    /// Starts direct unpack for downloads in which the headers of a set's first volume are now
    /// written (`livewrite::take_new_heads`), on the same terms as when a volume completes:
    /// the set is then read while it downloads, from before its first volume is complete.
    /// One that cannot start yet (direct rename still running, another unpack busy) is tried
    /// again a little later; its completed volumes start it as before anyway.
    fn drain_live_heads(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        let now = crate::liveorder::now_ms();
        let due: Vec<i32> = {
            let mut pending = PENDING_HEADS.lock().unwrap_or_else(|e| e.into_inner());
            for id in crate::livewrite::take_new_heads() {
                if !pending.iter().any(|(p, _)| *p == id) {
                    pending.push((id, 0));
                }
            }
            if pending.iter().all(|(_, at)| *at > now) {
                return;
            }
            let (due, later): (Vec<_>, Vec<_>) = pending.drain(..).partition(|(_, at)| *at <= now);
            *pending = later;
            due.into_iter().map(|(id, _)| id).collect()
        };
        let mut again = Vec::new();
        host.with_locked_queue(|dq_ptr| {
            for id in due {
                let nzb_ptr = host.queue_find_by_id(dq_ptr, id);
                if nzb_ptr.is_none() {
                    continue;
                }
                let nzb = host.nzb_snap(nzb_ptr, false);
                if nzb.direct_unpack_status != DirectUnpackStatus::None || nzb.is_download_completed {
                    // Running (it picks the volume up itself), done, or the download is over.
                    continue;
                }
                if !live_extraction_allowed(host, nzb_ptr, &nzb) {
                    // Never extracted live (unpack off for it, nothing written to disk).
                    continue;
                }
                // The set's extraction order from now on (by the names it has now, after a
                // direct rename), also while direct rename runs.
                {
                    if let Some(order) = head_name(id)
                        .and_then(|first| live_set_order(host, nzb_ptr, Some(&first)))
                    {
                        crate::liveorder::set_order(id, &order);
                    }
                }
                if nzb.direct_rename_status != DirectRenameStatus::Running {
                    host.prepost_queue_changed(
                        dq_ptr,
                        crate::queuesubsys_host::ASPECT_LIVE_HEAD,
                        nzb_ptr,
                        FileRef::NONE,
                    );
                    if host.nzb_snap(nzb_ptr, false).direct_unpack_status != DirectUnpackStatus::None {
                        continue;
                    }
                }
                again.push((id, now + LIVE_HEAD_RETRY_MS));
            }
        });
        if !again.is_empty() {
            let mut pending = PENDING_HEADS.lock().unwrap_or_else(|e| e.into_inner());
            for (id, at) in again {
                if !pending.iter().any(|(p, _)| *p == id) {
                    pending.push((id, at));
                }
            }
        }
    }

    /// Downloads whose file names already show a set direct unpack will extract (its first
    /// volume and the rest) take it in extraction order from their start: the names are only a
    /// download preference, direct unpack itself starts from the headers once they are written
    /// (`drain_live_heads`), and registers the order again then by the names it has by then.
    fn order_by_names(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        let now = crate::liveorder::now_ms();
        if now < NEXT_ORDER_BY_NAME_MS.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        NEXT_ORDER_BY_NAME_MS.store(now + 500, std::sync::atomic::Ordering::Relaxed);
        if !host.opts().direct_unpack {
            return;
        }
        host.with_locked_queue(|dq_ptr| {
            let mut seen = ORDER_BY_NAME_SEEN.lock().unwrap_or_else(|e| e.into_inner());
            let mut present = Vec::new();
            for i in 0..host.queue_len(dq_ptr) {
                let nzb_ptr = host.queue_at(dq_ptr, i);
                let nzb = host.nzb_snap(nzb_ptr, false);
                present.push(nzb.id);
                if seen.contains(&nzb.id) {
                    continue;
                }
                if nzb.direct_unpack_status != DirectUnpackStatus::None
                    || crate::liveorder::ranks(nzb.id).is_some()
                {
                    seen.push(nzb.id);
                    continue;
                }
                // (looked at again while it may not be extracted live: that can change)
                if !live_extraction_allowed(host, nzb_ptr, &nzb) {
                    continue;
                }
                seen.push(nzb.id);
                if let Some(order) = live_set_order(host, nzb_ptr, None) {
                    crate::liveorder::set_order(nzb.id, &order);
                }
            }
            seen.retain(|id| present.contains(id));
        });
    }

    /// Asks again for articles a live extraction will wait for that run far longer than the
    /// others (see `liveorder`): the slow request is dropped and the article handed out anew,
    /// which on another connection usually comes in a second.
    fn restart_live_stragglers(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        if !crate::liveorder::any() {
            return;
        }
        let now = crate::liveorder::now_ms();
        if now < NEXT_STRAGGLER_CHECK_MS.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        NEXT_STRAGGLER_CHECK_MS.store(
            now + STRAGGLER_CHECK_MS,
            std::sync::atomic::Ordering::Relaxed,
        );
        let Some(median) = crate::liveorder::median_ms() else {
            return;
        };
        let after = crate::liveorder::hedge_threshold(median);
        let slow: Vec<_> = self
            .active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|ad| {
                ad.status() == crate::queuesubsys_host::AD_RUNNING
                    && !ad.stopped.load(std::sync::atomic::Ordering::SeqCst)
                    && ad
                        .running_since_ms()
                        .is_some_and(|t| now.saturating_sub(t) >= after)
            })
            .cloned()
            .collect();
        if slow.is_empty() {
            return;
        }
        host.with_locked_queue(|_| {
            let mut orders: Vec<(i32, Option<LiveOrder>)> = Vec::new();
            for ad in slow {
                let f = host.file_snap(ad.file_ptr);
                if f.deleted {
                    continue;
                }
                let nzb = host.nzb_snap(f.nzb_ptr, false);
                let Some(order) = cached_live_order(&mut orders, host, f.nzb_ptr, nzb.id) else {
                    continue;
                };
                if order.window_rank(&f.filename).is_none() {
                    continue;
                }
                if !crate::liveorder::note_restart(nzb.id, ad.article_ptr) {
                    continue;
                }
                // Finished meanwhile: nothing to ask again.
                if ad.status() != crate::queuesubsys_host::AD_RUNNING {
                    continue;
                }
                let ran = now.saturating_sub(ad.running_since_ms().unwrap_or(now));
                // Its body read already (decoded and written now): not stopped.
                if !ad.restart_for_live() {
                    continue;
                }
                let mut msg = b"Asking again for ".to_vec();
                msg.extend_from_slice(ad.info_name());
                msg.extend_from_slice(
                    format!(
                        ": running {:.1} s where articles take {:.1} s, and the live extraction will wait for it",
                        ran as f64 / 1000.0,
                        median as f64 / 1000.0
                    )
                    .as_bytes(),
                );
                host.nzb_print_message(f.nzb_ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
            }
        });
    }

    fn get_next_first_article(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> Option<(
        crate::queuesubsys_host::FileSnap,
        crate::queuesubsys_host::ArticleSnap,
    )> {
        let flen = host.nzb_file_len(nzb.ptr);
        for i in 0..flen {
            let fp = host.nzb_file_at(nzb.ptr, i);
            let mut f = host.file_snap(fp);
            if !f.filename_confirmed {
                f = self.ensure_articles_loaded(host, opts, nzb, fp);
                if f.articles_len > 0 {
                    let a0 = host.article_at(fp, 0);
                    if a0.status == crate::queuesubsys_host::AI_UNDEFINED {
                        host.nzb_set_direct_rename_status(nzb.ptr, DirectRenameStatus::Running);
                        return Some((f, a0));
                    }
                }
            }
        }
        host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_ALL_FIRST, 1);
        None
    }

    fn start_article_download(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb: &crate::queuesubsys_host::NzbSnap,
        file: &crate::queuesubsys_host::FileSnap,
        art: &crate::queuesubsys_host::ArticleSnap,
        conn_ptr: usize,
    ) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Starting new ArticleDownloader",
        );
        let attach_analyzer = art.part_number == 1
            && !opts.raw_article
            && (opts.direct_rename
                || opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE);

        let mut info_name = nzb.name.clone();
        info_name.push(crate::paths::PATH_SEPARATOR);
        info_name.extend_from_slice(&file.filename);
        info_name
            .extend_from_slice(format!(" [{}/{}]", art.part_number, file.articles_len).as_bytes());

        host.start_article_download(file.ptr, art.ptr, conn_ptr, attach_analyzer, &info_name);
    }

    fn unpause_streaming_par_volumes(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_ptr: NzbRef,
        nzb_id: i32,
    ) {
        loop {
            let ids = crate::parchecker::streaming_session_take_unpause_ids_bounded(nzb_id, 32);
            if ids.is_empty() {
                break;
            }
            let flen = host.nzb_file_len(nzb_ptr);
            for uid in ids {
                for i in 0..flen {
                    let qf_ptr = host.nzb_file_at(nzb_ptr, i);
                    let qf = host.file_snap(qf_ptr);
                    if (qf.id as u64) == uid && qf.paused && !qf.deleted {
                        host.file_set_int(qf_ptr, crate::queuesubsys_host::FILE_INT_PAUSED, 0);
                        host.file_set_int(
                            qf_ptr,
                            crate::queuesubsys_host::FILE_INT_EXTRA_PRIORITY,
                            1,
                        );
                        break;
                    }
                }
            }
        }
    }

    fn capture_live_completed_file(
        nzb: &crate::queuesubsys_host::NzbSnap,
        file: &crate::queuesubsys_host::FileSnap,
        articles: &[crate::queuesubsys_host::ArticleSnap],
        filename: &[u8],
        successful: bool,
    ) -> Result<(), crate::parchecker::RepairError> {
        // the downloaded byte ranges, adjacent articles merged (par-rs
        // normalize_ranges): a damaged 5 GB volume of 7,000 articles with a
        // few holes is a handful of ranges, not 7,000
        let accepted = if successful {
            Vec::new()
        } else {
            downloaded_ranges(articles.iter().map(|a| {
                (
                    a.status == crate::queuesubsys_host::AI_FINISHED,
                    a.segment_offset,
                    a.segment_size as i64,
                )
            }))
        };
        let count = accepted.len();
        if count > 4096 {
            return Err(crate::parchecker::RepairError::Io {
                context: "capturing live PAR completion".into(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "{} arrived in {} separate pieces between its missing articles (live repair takes up to 4096 per file); it is repaired after the download",
                        String::from_utf8_lossy(filename),
                        count
                    ),
                ),
            });
        }
        let bytes = 1024
            + 2 * (nzb.dest_dir.len() + filename.len() + file.hash16k.len())
            + count * 6 * std::mem::size_of::<crate::parchecker::ByteRange>();
        crate::parchecker::streaming_session_queue_completed_file(nzb.id, bytes as u64, || {
            let path = join_path(&nzb.dest_dir, filename);
            Ok(crate::parchecker::LiveCompletedFile {
                nzb_id: nzb.id,
                base_dir: crate::fsops::path(&nzb.dest_dir).to_path_buf(),
                path: crate::fsops::path(&path).to_path_buf(),
                name: filename.to_vec(),
                hash16k: file.hash16k.clone(),
                par_file: file.par_file
                    || crate::parparser::parse_par_filename(filename, true).is_some(),
                successful,
                accepted,
                queued_id: Some(file.id as u64),
            })
        })
    }

    /// The message for live PAR that could not start or catch up; a memory
    /// refusal is told in MB with the ParBuffer it would take.
    fn live_par_deferred_message(error: &crate::parchecker::RepairError) -> String {
        if let crate::parchecker::RepairError::StreamingMemoryLimit {
            context: "bounded live PAR startup",
            required_bytes,
            limit_bytes,
        } = error
        {
            let mb = |b: u64| b.div_ceil(1024 * 1024);
            return format!(
                "Live PAR catch-up deferred: this release's par2 blocks need {} MB of ParBuffer to repair during the download, {} MB is available; set ParBuffer to {} MB or more for live repair. Ordinary repair remains available.",
                mb(*required_bytes),
                mb(*limit_bytes),
                mb(*required_bytes)
            );
        }
        format!("Live PAR catch-up deferred: {error}. Ordinary repair remains available.")
    }

    fn report_live_completion_error(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_id: i32,
        error: &crate::parchecker::RepairError,
    ) {
        if matches!(error, crate::parchecker::RepairError::Cancelled) {
            return;
        }
        host.with_locked_queue(|dq| {
            let nzb = host.queue_find_by_id(dq, nzb_id);
            if nzb.is_some() {
                let message = Self::live_par_deferred_message(error);
                host.nzb_print_message(nzb, crate::queuesubsys_host::MK_DETAIL, message.as_bytes());
            }
        });
    }

    fn resolve_live_completed_file(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        file: &mut crate::parchecker::LiveCompletedFile,
    ) -> Result<(), crate::parchecker::RepairError> {
        let mut result = Err(crate::parchecker::RepairError::Cancelled);
        host.with_locked_queue(|dq| {
            let ptr = host.queue_find_by_id(dq, file.nzb_id);
            if ptr.is_none() || !live_par_syndromes(&host.opts()) {
                return;
            }
            let nzb = host.nzb_snap(ptr, false);
            if !live_start_context_matches(&nzb, &file.base_dir, true) {
                return;
            }
            let Some(id) = file.queued_id else {
                return;
            };
            let completed = (0..host.nzb_completed_files_len(ptr))
                .map(|i| host.nzb_completed_file_at(ptr, i))
                .find(|cf| cf.id as u64 == id);
            let Some(current) = completed else {
                return;
            };
            if file.successful && current.status != crate::queuesubsys_host::CF_SUCCESS {
                return;
            }
            file.name = current.filename;
            file.hash16k = current.hash16k;
            file.base_dir = crate::fsops::path(&nzb.dest_dir).to_path_buf();
            let path = join_path(&nzb.dest_dir, &file.name);
            file.path = crate::fsops::path(&path).to_path_buf();
            result = Ok(());
        });
        result
    }

    fn start_checked_live_session(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        file: &crate::parchecker::LiveCompletedFile,
        writers: std::sync::Arc<crate::parlive::WriteCoordinator>,
    ) -> Result<(), crate::parchecker::RepairError> {
        use crate::parchecker::{prepare_live_start, RepairError};
        use std::os::unix::fs::MetadataExt;
        let root_identity = || -> Result<(u64, u64), RepairError> {
            let metadata =
                std::fs::symlink_metadata(&file.base_dir).map_err(|source| RepairError::Io {
                    context: "live source root identity".into(),
                    source,
                })?;
            if !metadata.is_dir() {
                return Err(RepairError::Cancelled);
            }
            Ok((metadata.dev(), metadata.ino()))
        };
        let root = root_identity()?;
        let attempt = self.live_startups.begin()?;
        let check_stamp = |dq| -> Result<NzbRef, RepairError> {
            attempt.ensure_current()?;
            if self.stopped.load(std::sync::atomic::Ordering::Acquire) || host.self_is_stopped() {
                return Err(RepairError::Cancelled);
            }
            let ptr = host.queue_find_by_id(dq, file.nzb_id);
            if ptr.is_none() || !live_par_syndromes(&host.opts()) {
                return Err(RepairError::Cancelled);
            }
            let nzb = host.nzb_snap(ptr, false);
            if !live_start_context_matches(&nzb, &file.base_dir, true)
                || file.queued_id.is_none_or(|id| {
                    !host.live_completed_matches(ptr, id, &file.name, file.successful)
                })
            {
                return Err(RepairError::Cancelled);
            }
            Ok(ptr)
        };
        let pool = crate::parchecker::effective_live_pool_bytes();
        let mut metadata = prepare_live_start(&file.path, &file.base_dir, pool, writers)?;
        let catalog = metadata.catalog_required_bytes()?;
        let mut hint = crate::parchecker::LivePlanHint::default();
        let facts = host.with_locked_queue(|dq| {
            let ptr = check_stamp(dq)?;
            hint = Self::live_plan_hint(host, ptr, &file.name);
            host.live_namespace_capture(ptr, file.nzb_id, &file.base_dir, catalog, pool)
        })?;
        metadata.set_plan_hint(hint);
        let mut prepared = metadata.finish_tracked(&file.path, facts, &attempt)?;
        if root_identity()? != root {
            return Err(RepairError::Cancelled);
        }
        let installed = host.with_locked_queue(|dq| {
            let ptr = check_stamp(dq)?;
            prepared.check_and_install(file.nzb_id, |proof| {
                host.live_namespace_visit(ptr, |row| {
                    proof.recheck_row(row.queue_id, row.flags, row.fields)
                })
            })
        })?;
        installed.finish(file.nzb_id);
        Ok(())
    }

    /// The recovery volumes of the set of `par_name` in the queue and the job's failures so
    /// far, for sizing live PAR's recovery slots. The caller holds the queue lock.
    fn live_plan_hint(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        ptr: NzbRef,
        par_name: &[u8],
    ) -> crate::parchecker::LivePlanHint {
        let nzb = host.nzb_snap(ptr, false);
        let positive = |v: i64| u64::try_from(v).unwrap_or(0);
        let failed = positive(nzb.current_failed_size - nzb.par_current_failed_size);
        let succeeded = positive(nzb.current_success_size - nzb.par_current_success_size);
        let mut hint = crate::parchecker::LivePlanHint {
            volumes: Vec::new(),
            failed_bytes: failed,
            failed_articles: positive(i64::from(nzb.current_failed_articles)),
            done_bytes: failed + succeeded,
            total_bytes: positive(nzb.size - nzb.par_size),
        };
        for i in 0..host.nzb_file_len(ptr).min(4096) {
            let file = host.file_snap(host.nzb_file_at(ptr, i));
            if file.deleted
                || !crate::parparser::same_par_collection(
                    par_name,
                    &file.filename,
                    file.filename_confirmed,
                )
            {
                continue;
            }
            let Some((first_exponent, count)) = crate::parchecker::volume_range(&file.filename)
            else {
                continue;
            };
            hint.volumes.push(crate::parchecker::LiveQueuedVolume {
                id: file.id as u64,
                first_exponent,
                count,
                size: positive(file.size),
                articles: u32::try_from(file.total_articles).unwrap_or(0),
                paused: file.paused,
            });
        }
        hint
    }

    fn drain_live_par_completed_files(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        for id in crate::parchecker::streaming_session_pending_completed_jobs() {
            let Some(_owner) = crate::parchecker::streaming_session_claim_completion_drain(id)
            else {
                continue;
            };
            while !self.stopped.load(std::sync::atomic::Ordering::Acquire) {
                let Some(result) =
                    crate::parchecker::streaming_session_process_completed_file_with_start(
                        id,
                        |file| Self::resolve_live_completed_file(host, file),
                        |file, writers| self.start_checked_live_session(host, file, writers),
                    )
                else {
                    break;
                };
                match result {
                    Ok(true) => self.reconcile_live_par_startup(host, id),
                    Ok(false) => {}
                    Err(error) if crate::parchecker::streaming_session_capture_busy(&error) => {
                        break
                    }
                    Err(error) => Self::report_live_completion_error(host, id, &error),
                }
            }
            if !self.stopped.load(std::sync::atomic::Ordering::Acquire) {
                host.with_locked_queue(|dq| {
                    let ptr = host.queue_find_by_id(dq, id);
                    if ptr.is_some() {
                        crate::postproc::direct_unpack_try_live_par_repair(
                            host,
                            &host.nzb_snap(ptr, false),
                        );
                    }
                });
            }
        }
    }

    fn reconcile_live_par_startup(&self, host: &crate::queuesubsys_host::QueueSubsysHost, id: i32) {
        let Some(par_name) = crate::parchecker::streaming_session_par_filename(id) else {
            return;
        };
        let mut live = false;
        host.with_locked_queue(|dq| {
            let ptr = host.queue_find_by_id(dq, id);
            if ptr.is_none() || !live_par_syndromes(&host.opts()) {
                return;
            }
            let nzb = host.nzb_snap(ptr, false);
            if nzb.deleting {
                return;
            }
            live = true;
            let count = host.nzb_file_len(ptr).min(4096);
            for i in 0..count {
                let file = host.file_snap(host.nzb_file_at(ptr, i));
                if file.deleted
                    || !crate::parparser::same_par_collection(
                        &par_name,
                        &file.filename,
                        file.filename_confirmed,
                    )
                {
                    continue;
                }
                if let Some((_, blocks)) =
                    crate::parparser::parse_par_filename(&file.filename, file.filename_confirmed)
                {
                    if blocks != 0
                        && !crate::parchecker::streaming_session_register_queued_volume(
                            id,
                            file.id as u64,
                            blocks.max(1),
                            file.paused,
                        )
                    {
                        break;
                    }
                }
            }
        });
        if !live {
            crate::parchecker::streaming_session_forget(id);
            return;
        }
        // Walk completed history one owned item at a time. Large prefixes never
        // require a second queue-sized snapshot or a long global queue lock.
        let mut index = 0;
        loop {
            if self.stopped.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            let mut done = false;
            let mut captured = Ok(false);
            host.with_locked_queue(|dq| {
                let ptr = host.queue_find_by_id(dq, id);
                if ptr.is_none() || !live_par_syndromes(&host.opts()) {
                    done = true;
                    return;
                }
                let nzb = host.nzb_snap(ptr, false);
                if nzb.deleting || index >= host.nzb_completed_files_len(ptr) {
                    done = true;
                    return;
                }
                let file = host.nzb_completed_file_at(ptr, index);
                index += 1;
                if file.par_file
                    && (file.filename == par_name
                        || !crate::parparser::same_par_collection(&par_name, &file.filename, true))
                {
                    return;
                }
                let successful = file.status == crate::queuesubsys_host::CF_SUCCESS;
                let partial = file.status == crate::queuesubsys_host::CF_PARTIAL;
                let count = if partial { 4096 } else { 0 };
                let bytes = 1024
                    + 2 * (nzb.dest_dir.len() + file.filename.len() + file.hash16k.len())
                    + count * 6 * std::mem::size_of::<crate::parchecker::ByteRange>();
                captured = crate::parchecker::streaming_session_queue_completed_file(
                    id,
                    bytes as u64,
                    || {
                        let accepted = if partial {
                            let length = crate::parchecker::streaming_session_source_length(
                                id,
                                &file.filename,
                                &file.hash16k,
                            )
                            .ok_or(
                                crate::parchecker::RepairError::UnknownFile(
                                    crate::parchecker::FileId([0; 16]),
                                ),
                            )?;
                            crate::pardamage::completed_accepted_ranges(id, &file.filename, length)
                                .or_else(|| {
                                    crate::pardamage::completed_accepted_ranges(
                                        id,
                                        &file.origname,
                                        length,
                                    )
                                })
                                .ok_or_else(|| crate::parchecker::RepairError::Io {
                                    context: "live partial-file history".into(),
                                    source: std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        "accepted range history unavailable",
                                    ),
                                })?
                        } else {
                            Vec::new()
                        };
                        let path = join_path(&nzb.dest_dir, &file.filename);
                        Ok(crate::parchecker::LiveCompletedFile {
                            nzb_id: id,
                            base_dir: crate::fsops::path(&nzb.dest_dir).to_path_buf(),
                            path: crate::fsops::path(&path).to_path_buf(),
                            name: file.filename.clone(),
                            hash16k: file.hash16k.clone(),
                            par_file: file.par_file,
                            successful,
                            accepted,
                            queued_id: Some(file.id as u64),
                        })
                    },
                )
                .map(|()| true);
            });
            if done {
                break;
            }
            match captured {
                Ok(true) => {
                    if let Some(Err(error)) =
                        crate::parchecker::streaming_session_process_completed_file_with_start(
                            id,
                            |file| Self::resolve_live_completed_file(host, file),
                            |file, writers| self.start_checked_live_session(host, file, writers),
                        )
                    {
                        Self::report_live_completion_error(host, id, &error);
                    }
                }
                Ok(false) => {}
                Err(error) => Self::report_live_completion_error(host, id, &error),
            }
            Self::drain_live_par_articles(host);
        }
        // Accepted in-progress bytes may still be in the cache. Snapshot a few
        // at a time; uncached data waits for its own committed completion event.
        let mut ids = Vec::new();
        host.with_locked_queue(|dq| {
            let ptr = host.queue_find_by_id(dq, id);
            if ptr.is_none() {
                return;
            }
            for i in 0..host.nzb_file_len(ptr).min(32768) {
                let file = host.file_snap(host.nzb_file_at(ptr, i));
                if !file.deleted && !file.par_file {
                    ids.push(file.id);
                }
            }
        });
        for file_id in ids {
            let mut article = 0;
            loop {
                if self.stopped.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                let mut done = false;
                host.with_locked_queue(|dq| {
                    let ptr = host.queue_find_by_id(dq, id);
                    if ptr.is_none() || !live_par_syndromes(&host.opts()) {
                        done = true;
                        return;
                    }
                    let nzb = host.nzb_snap(ptr, false);
                    if nzb.deleting {
                        done = true;
                        return;
                    }
                    let file = (0..host.nzb_file_len(ptr))
                        .map(|i| host.file_snap(host.nzb_file_at(ptr, i)))
                        .find(|file| file.id == file_id && !file.deleted);
                    let Some(file) = file else {
                        done = true;
                        return;
                    };
                    for _ in 0..4 {
                        if article >= file.articles_len {
                            done = true;
                            break;
                        }
                        let part = host.article_at(file.ptr, article);
                        article += 1;
                        if part.status == crate::queuesubsys_host::AI_FINISHED
                            && part.segment_size > 0
                            && part.segment_offset >= 0
                        {
                            let written = live_written_file(&file);
                            if let Some(Err(_error)) =
                                crate::parchecker::streaming_session_defer_committed_article(
                                    id,
                                    (Some(file.id), &file.filename, &file.hash16k),
                                    part.segment_offset as u64,
                                    part.segment_size as u64,
                                    part.crc,
                                    written.as_deref(),
                                    || {
                                        host.article_cache_snapshot_segment(
                                            file.ptr,
                                            part.ptr,
                                            part.segment_size as usize,
                                        )
                                    },
                                )
                            {
                                Self::report_live_par_warning(host, ptr, id);
                            }
                        }
                    }
                });
                Self::drain_live_par_articles(host);
                if done {
                    break;
                }
            }
        }
    }

    fn report_live_par_warning(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_ptr: NzbRef,
        nzb_id: i32,
    ) {
        if let Some(warning) = crate::parchecker::streaming_session_take_warning(nzb_id) {
            let message = format!(
                "Live PAR accumulation deferred: {warning}. Ordinary repair remains available."
            );
            host.nzb_print_message(
                nzb_ptr,
                crate::queuesubsys_host::MK_DETAIL,
                message.as_bytes(),
            );
        }
    }

    /// Run hashing/folding outside the daemon's global queue lock, then refresh
    /// queue identity before applying the cheap scheduling decision.
    fn drain_live_par_articles(host: &crate::queuesubsys_host::QueueSubsysHost) {
        for id in crate::parchecker::streaming_session_pending_article_jobs() {
            // Operation errors are retained by the session and surfaced once below.
            let failed = matches!(
                crate::parchecker::streaming_session_drain_articles(id),
                Some(Err(_))
            );
            crate::parchecker::streaming_session_kick_backfill(id);
            let polled = crate::parchecker::streaming_session_poll(id, false);
            host.with_locked_queue(|dq| {
                let ptr = host.queue_find_by_id(dq, id);
                if ptr.is_none() || !live_par_syndromes(&host.opts()) {
                    crate::parchecker::streaming_session_forget(id);
                    return;
                }
                let nzb = host.nzb_snap(ptr, false);
                if nzb.deleting {
                    return;
                }
                if let Some(st) = polled {
                    let snapshot = crate::liveparlog::Snapshot {
                        verified: st.verified,
                        damaged: st.damaged,
                        not_arrived: st.unarrived + st.partially_arrived,
                        total: st.total,
                        recovery_ready: st.ready_recovery,
                        recovery_in_flight: st.unpaused_in_flight,
                        recovery_needed: st.needed_recovery_blocks,
                        repair_possible: st.early_cutover_possible,
                    };
                    if let Some(tail) = crate::liveparlog::line_if_due(id, snapshot) {
                        let mut msg = b"Live par2 for ".to_vec();
                        msg.extend_from_slice(&nzb.name);
                        msg.extend_from_slice(tail.as_bytes());
                        host.nzb_print_message(ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
                    }
                }
                if failed || crate::parchecker::streaming_session_last_error(id).is_some() {
                    Self::report_live_par_warning(host, ptr, id);
                }
                Self::unpause_streaming_par_volumes(host, ptr, id);
                crate::postproc::direct_unpack_try_live_par_repair(host, &nzb);
            });
        }
    }

    pub(crate) fn article_completed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        ad_ptr: usize,
    ) {
        let _callback_sample = self
            .callback_metrics
            .as_ref()
            .map(|metrics| metrics.sample());
        host.log(crate::queuesubsys_host::MK_DEBUG, b"Article downloaded");
        let ad = host.ad_snap(ad_ptr);
        // How long articles take, for telling a slow one (see `restart_live_stragglers`).
        if ad.status == crate::queuesubsys_host::AD_FINISHED {
            if let Some(since) = self.find_ad(ad_ptr).and_then(|a| a.running_since_ms()) {
                crate::liveorder::note_duration(crate::liveorder::now_ms().saturating_sub(since));
            }
        }
        let file_ptr = ad.file_ptr;
        let mut complete_file_parts = false;

        host.with_locked_queue(|dq_ptr| {
            let opts = host.opts();
            let mut file = host.file_snap(file_ptr);
            let mut nzb = host.nzb_snap(file.nzb_ptr, false);
            let articles_len = file.articles_len;
            let Some((art_idx, mut art)) = host.file_find_article(file_ptr, ad.article_ptr) else {
                self.delete_downloader(host, host.self_download_queue_ptr(), ad_ptr, false);
                return;
            };

            let mut retry = false;
            let mut file_completed = false;

            let check_aligned = ad.status == crate::queuesubsys_host::AD_FINISHED
                && art.dupe_fallback_round > 0
                && ad.decoded_file_size > 0;
            let misplaced = check_aligned
                && !crate::dupefallback::segment_aligned_at(
                    host,
                    file_ptr,
                    articles_len,
                    art_idx,
                    &art,
                    file.decoded_file_size,
                );
            if misplaced {
                self.discard_article_segment(host, &mut file, &art);
                let mut msg = b"Discarding misaligned article ".to_vec();
                msg.extend_from_slice(&file.filename);
                msg.extend_from_slice(
                    format!(" [{}/{}] from duplicate", art.part_number, articles_len).as_bytes(),
                );
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
            }

            if ad.status == crate::queuesubsys_host::AD_FINISHED && !misplaced {
                art.status = crate::queuesubsys_host::AI_FINISHED;
                host.article_edit(art.ptr, crate::qarticle::ArticleEdit::Status(crate::queuesubsys_host::AI_FINISHED));
                if art.dupe_fallback_round == 0
                    || (!art.dupe_original_message_id.is_empty()
                        && art.message_id == art.dupe_original_message_id)
                {
                    nzb.dupe_failed_run = 0;
                    host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_DUPE_FAILED_RUN, 0);
                }

                let (fs, ns, ps, fa, na) = apply_success_article_counters(
                    i64::from(art.size),
                    file.par_file,
                    file.success_size,
                    nzb.current_success_size,
                    nzb.par_current_success_size,
                    file.success_articles,
                    nzb.current_success_articles,
                );
                file.success_size = fs;
                nzb.current_success_size = ns;
                nzb.par_current_success_size = ps;
                file.success_articles = fa;
                nzb.current_success_articles = na;
                host.file_set_i64(file.ptr, crate::queuesubsys_host::FILE_I64_SUCCESS_SIZE, fs);
                host.nzb_set_i64(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_I64_CUR_SUCCESS_SIZE,
                    ns,
                );
                host.nzb_set_i64(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_I64_PAR_CUR_SUCCESS_SIZE,
                    ps,
                );
                host.file_set_int(
                    file.ptr,
                    crate::queuesubsys_host::FILE_INT_SUCCESS_ARTICLES,
                    fa,
                );
                host.nzb_set_int(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_INT_CUR_SUCCESS_ARTICLES,
                    na,
                );

                let previous_file_size = file.decoded_file_size;
                if crate::dupefallback::merge_decoded_file_size_host(
                    host,
                    &mut file,
                    ad.decoded_file_size,
                ) {
                    let mut msg = b"Articles of ".to_vec();
                    msg.extend_from_slice(&file.filename);
                    msg.extend_from_slice(
                        format!(
                            " declare different file sizes ({} vs {}); ignoring the declared size",
                            previous_file_size, ad.decoded_file_size
                        )
                        .as_bytes(),
                    );
                    host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
                }

                let (_, source_donors) = host.article_pinned_sources(art.ptr);
                let donor_id = select_dupe_donor_id(
                    art.dupe_fallback_round,
                    &source_donors,
                    &art.message_id,
                    &art.dupe_original_message_id,
                );
                art.dupe_donor_id = donor_id;
                host.article_edit(art.ptr, crate::qarticle::ArticleEdit::DonorId(donor_id));

                let (rec, next_file_rec, next_nzb_rec, trig_cutover) =
                    eval_dupe_recovery_and_cutover(
                        art.dupe_fallback_round,
                        art.dupe_proactive,
                        &art.message_id,
                        &art.dupe_original_message_id,
                        file.dupe_recovered_articles,
                        nzb.dupe_recovered_articles,
                        file.dupe_cutover,
                        crate::dupefallback::CUTOVER_THRESHOLD,
                    );
                if rec {
                    if file.par_file {
                        note_dupe_par_article(host, nzb.ptr, file.id, true);
                    }
                    file.dupe_recovered_articles = next_file_rec;
                    nzb.dupe_recovered_articles = next_nzb_rec;
                    host.file_set_int(
                        file.ptr,
                        crate::queuesubsys_host::FILE_INT_DUPE_RECOVERED_ARTICLES,
                        next_file_rec,
                    );
                    host.nzb_set_int(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_INT_DUPE_RECOVERED_ARTICLES,
                        next_nzb_rec,
                    );
                    if trig_cutover {
                        file.dupe_cutover = true;
                        host.file_set_int(
                            file.ptr,
                            crate::queuesubsys_host::FILE_INT_DUPE_CUTOVER,
                            1,
                        );
                        let mut msg = b"Leading with duplicate collections for ".to_vec();
                        msg.extend_from_slice(&file.filename);
                        msg.extend_from_slice(b" (primary is missing many articles)");
                        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
                    }
                }

                crate::dupefallback::register_lead_success_at(
                    host,
                    &mut file,
                    articles_len,
                    art_idx,
                    &art,
                );

                if file.dupe_recovered_articles > 0 && ad.decoded_file_size > 0 {
                    self.demote_misaligned_dupe_neighbors(
                        host,
                        &mut nzb,
                        &mut file,
                        articles_len,
                        art_idx,
                    );
                }

                if !file.par_file
                    && art.segment_offset >= 0
                    && art.segment_size > 0
                    && crate::parchecker::streaming_session_has_session(nzb.id)
                {
                    if live_par_syndromes(&opts) {
                        let written = live_written_file(&file);
                        if let Some(Err(_error)) = crate::parchecker::streaming_session_defer_committed_article(
                            nzb.id, (Some(file.id), &file.filename, &file.hash16k), art.segment_offset as u64,
                            art.segment_size as u64, art.crc, written.as_deref(),
                            || host.article_cache_snapshot_segment(file.ptr, art.ptr, art.segment_size as usize),
                        ) {
                            Self::report_live_par_warning(host, nzb.ptr, nzb.id);
                        }
                    }
                }
            } else if (ad.status == crate::queuesubsys_host::AD_FAILED || misplaced)
                && self.with_donor_cache(|cache| {
                    crate::dupefallback::try_fallback_host(
                        cache,
                        host,
                        &opts,
                        dq_ptr,
                        &mut nzb,
                        &mut file,
                        art_idx,
                        &mut art,
                    )
                })
            {
                art.status = crate::queuesubsys_host::AI_UNDEFINED;
                host.article_edit(art.ptr, crate::qarticle::ArticleEdit::Status(crate::queuesubsys_host::AI_UNDEFINED));
                retry = true;
                if art.part_number == 1 {
                    nzb.all_first = false;
                    host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_ALL_FIRST, 0);
                }
            } else if ad.status == crate::queuesubsys_host::AD_FAILED
                || ad.status == crate::queuesubsys_host::AD_FATAL_ERROR
                || misplaced
            {
                art.status = crate::queuesubsys_host::AI_FAILED;
                host.article_edit(art.ptr, crate::qarticle::ArticleEdit::Status(crate::queuesubsys_host::AI_FAILED));
                let (dfr, ff_sz, nf_sz, npf_sz, ff_art, nf_art) = apply_failed_article_counters(
                    art.part_number,
                    file.par_file as i32,
                    art.size,
                    nzb.dupe_failed_run,
                    file.failed_size,
                    nzb.current_failed_size,
                    nzb.par_current_failed_size,
                    file.failed_articles,
                    nzb.current_failed_articles,
                );
                nzb.dupe_failed_run = dfr;
                file.failed_articles = ff_art;
                nzb.current_failed_articles = nf_art;
                file.failed_size = ff_sz;
                nzb.current_failed_size = nf_sz;
                nzb.par_current_failed_size = npf_sz;
                host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_DUPE_FAILED_RUN, dfr);
                host.file_set_int(
                    file.ptr,
                    crate::queuesubsys_host::FILE_INT_FAILED_ARTICLES,
                    ff_art,
                );
                host.nzb_set_int(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_INT_CUR_FAILED_ARTICLES,
                    nf_art,
                );
                host.file_set_i64(file.ptr, crate::queuesubsys_host::FILE_I64_FAILED_SIZE, ff_sz);
                host.nzb_set_i64(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_I64_CUR_FAILED_SIZE,
                    nf_sz,
                );
                host.nzb_set_i64(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_I64_PAR_CUR_FAILED_SIZE,
                    npf_sz,
                );

                if !file.par_file
                    && crate::parchecker::streaming_session_has_session(nzb.id)
                    && art.segment_size > 0
                    && art.segment_offset >= 0
                {
                    if live_par_syndromes(&opts) {
                        if let Some(Err(_error)) = crate::parchecker::streaming_session_defer_failure_for_queue(
                            nzb.id, (Some(file.id), &file.filename, &file.hash16k), art.segment_offset as u64, art.segment_size as u64,
                        ) {
                            Self::report_live_par_warning(host, nzb.ptr, nzb.id);
                        }
                    }
                }
            } else if ad.status == crate::queuesubsys_host::AD_RETRY {
                art.status = crate::queuesubsys_host::AI_UNDEFINED;
                host.article_edit(art.ptr, crate::qarticle::ArticleEdit::Status(crate::queuesubsys_host::AI_UNDEFINED));
                retry = true;
                if art.part_number == 1 {
                    nzb.all_first = false;
                    host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_ALL_FIRST, 0);
                }
            }

            if ad.status != crate::queuesubsys_host::AD_RETRY {
                host.ad_action(
                    ad_ptr,
                    crate::queuesubsys_host::AD_ACT_ADD_SERVER_STATS,
                    0,
                );
            }

            // Refresh paused state in case UnpauseStreamingParVolumes changed it
            file = host.file_snap(file_ptr);
            nzb = host.nzb_snap(file.nzb_ptr, false);

            if !retry {
                let (rf, rn, pz, comp, fc) = advance_article_completion_counters(
                    file.remaining_size,
                    nzb.remaining_size,
                    nzb.paused_size,
                    art.size,
                    file.paused,
                    file.completed_articles,
                    articles_len as i32,
                );
                file.remaining_size = rf;
                nzb.remaining_size = rn;
                nzb.paused_size = pz;
                file.completed_articles = comp;
                file.partial_changed = true;
                file_completed = fc;
                host.file_set_i64(file.ptr, crate::queuesubsys_host::FILE_I64_REMAINING_SIZE, rf);
                host.nzb_set_i64(nzb.ptr, crate::queuesubsys_host::NZB_I64_REMAINING_SIZE, rn);
                host.nzb_set_i64(nzb.ptr, crate::queuesubsys_host::NZB_I64_PAUSED_SIZE, pz);
                host.file_set_int(
                    file.ptr,
                    crate::queuesubsys_host::FILE_INT_COMPLETED_ARTICLES,
                    comp,
                );
                host.file_set_int(file.ptr, crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED, 1);
            }

            if !file.filename_confirmed
                && !misplaced
                && ad.status == crate::queuesubsys_host::AD_FINISHED
                && ad.has_article_filename
            {
                let use_from_article = opts.file_naming == crate::queuesubsys_host::NF_ARTICLE
                    || (opts.file_naming == crate::queuesubsys_host::NF_AUTO
                        && !crate::deobfuscation::is_excessively_obfuscated(&ad.article_filename)
                        && !nzb.many_dupe_files);
                if use_from_article {
                    host.file_set_str(
                        file.ptr,
                        crate::queuesubsys_host::FILE_STR_FILENAME,
                        Some(&ad.article_filename),
                    );
                    host.file_action(
                        file.ptr,
                        crate::queuesubsys_host::FILE_ACT_MAKE_VALID_FILENAME,
                    );
                    file = host.file_snap(file.ptr);
                }
                file.filename_confirmed = true;
                host.file_set_int(
                    file.ptr,
                    crate::queuesubsys_host::FILE_INT_FILENAME_CONFIRMED,
                    1,
                );
                let mut dupe_file_exists = false;
                if opts.dupe_check
                    && nzb.dupe_mode != DupeMode::Force
                    && !nzb.many_dupe_files
                {
                    let check_path = join_path(&nzb.dest_dir, &file.filename);
                    dupe_file_exists =
                        crate::fsops::file_exists(&check_path, &crate::fsops::posix_stat);
                }
                if dupe_file_exists {
                    let mut msg = b"File \"".to_vec();
                    msg.extend_from_slice(&file.filename);
                    msg.extend_from_slice(
                        b"\" seems to be duplicate, cancelling download and deleting file from queue",
                    );
                    host.log(crate::queuesubsys_host::MK_WARNING, &msg);
                    file_completed = false;
                    file.dupe_deleted = true;
                    host.file_set_int(file.ptr, crate::queuesubsys_host::FILE_INT_DUPE_DELETED, 1);
                    self.delete_queue_entry(host, dq_ptr, file.ptr);
                    file = host.file_snap(file.ptr);
                }
            }

            if ad.has_content_analyzer
                && !misplaced
                && ad.status == crate::queuesubsys_host::AD_FINISHED
            {
                let fc = if ad.analyzed_par_file {
                    crate::queuesubsys_host::FC_PAR2
                } else {
                    crate::queuesubsys_host::FC_OTHER
                };
                file.first_content = fc;
                host.file_set_int(file.ptr, crate::queuesubsys_host::FILE_INT_FIRST_CONTENT, fc);
                if opts.direct_rename {
                    crate::directrenamer::article_downloaded_host(
                        self,
                        host,
                        &opts,
                        dq_ptr,
                        file.ptr,
                        art.size,
                        ad_ptr,
                    );
                }
            }

            nzb = host.nzb_snap(file.nzb_ptr, false);
            let next_dl_size = nzb.downloaded_size + ad.downloaded_size;
            host.nzb_set_i64(
                nzb.ptr,
                crate::queuesubsys_host::NZB_I64_DOWNLOADED_SIZE,
                next_dl_size,
            );

            file = host.file_snap(file.ptr);
            let finished_before_cancel = file_completed && !file.deleted;

            self.check_health(host, &opts, dq_ptr, file.ptr);
            self.check_dead_download(host, &opts, dq_ptr, nzb.ptr);
            self.check_locked_archive(host, &opts, dq_ptr, nzb.ptr);

            file = host.file_snap(file.ptr);
            nzb = host.nzb_snap(file.nzb_ptr, false);

            let (fc_out, cp_out) = eval_file_completion_and_assembly(
                file_completed,
                nzb.parking,
                file.active_downloads,
                file.dupe_deleted,
                file.deleted,
                finished_before_cancel,
            );
            file_completed = fc_out;
            complete_file_parts = cp_out;

            if file_completed
                && file.success_articles == file.total_articles
                && (opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE
                    || file.dupe_recovered_articles > 0
                    || file.dupe_attempted_articles > 0)
            {
                let mut fresh_articles = snapshot_file_articles(host, file.ptr, file.articles_len);
                self.validate_completed_file_tiling(
                    host,
                    &mut nzb,
                    &mut file,
                    &mut fresh_articles,
                );
            }

            if !complete_file_parts {
                self.delete_downloader(host, host.self_download_queue_ptr(), ad_ptr, false);
            }
        });

        if complete_file_parts {
            if !host.ad_action(
                ad_ptr,
                crate::queuesubsys_host::AD_ACT_COMPLETE_FILE_PARTS,
                0,
            ) {
                host.with_locked_queue(|dq_ptr| {
                    let opts = host.opts();
                    let file = host.file_snap(file_ptr);
                    let nzb = host.nzb_snap(file.nzb_ptr, false);
                    let mut f_fa = file.failed_articles;
                    let mut f_fs = file.failed_size;
                    let mut n_fa = nzb.current_failed_articles;
                    let mut n_sa = nzb.current_success_articles;
                    let mut n_fs = nzb.current_failed_size;
                    let mut n_ss = nzb.current_success_size;
                    let mut np_fs = nzb.par_current_failed_size;
                    let mut np_ss = nzb.par_current_success_size;
                    apply_assembly_failure_counters(
                        file.success_articles,
                        file.success_size,
                        file.par_file,
                        &mut f_fa,
                        &mut f_fs,
                        &mut n_fa,
                        &mut n_sa,
                        &mut n_fs,
                        &mut n_ss,
                        &mut np_fs,
                        &mut np_ss,
                    );
                    host.file_set_int(
                        file.ptr,
                        crate::queuesubsys_host::FILE_INT_FAILED_ARTICLES,
                        f_fa,
                    );
                    host.file_set_int(
                        file.ptr,
                        crate::queuesubsys_host::FILE_INT_SUCCESS_ARTICLES,
                        0,
                    );
                    host.file_set_i64(
                        file.ptr,
                        crate::queuesubsys_host::FILE_I64_FAILED_SIZE,
                        f_fs,
                    );
                    host.file_set_i64(file.ptr, crate::queuesubsys_host::FILE_I64_SUCCESS_SIZE, 0);
                    host.nzb_set_int(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_INT_CUR_FAILED_ARTICLES,
                        n_fa,
                    );
                    host.nzb_set_int(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_INT_CUR_SUCCESS_ARTICLES,
                        n_sa,
                    );
                    host.nzb_set_i64(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_I64_CUR_FAILED_SIZE,
                        n_fs,
                    );
                    host.nzb_set_i64(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_I64_CUR_SUCCESS_SIZE,
                        n_ss,
                    );
                    host.nzb_set_i64(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_I64_PAR_CUR_FAILED_SIZE,
                        np_fs,
                    );
                    host.nzb_set_i64(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_I64_PAR_CUR_SUCCESS_SIZE,
                        np_ss,
                    );
                    host.file_action(
                        file.ptr,
                        crate::queuesubsys_host::FILE_ACT_FAIL_FINISHED_ARTICLES,
                    );
                    self.discard_temp_files(host, &opts, &file);
                    self.delete_downloader(host, dq_ptr, ad_ptr, true);
                });
                return;
            }

            host.file_set_int(
                file_ptr,
                crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED,
                0,
            );

            let (mut path, dest_dir, filename, articles, daf_mode) = {
                let mut tup = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), 0i32);
                host.with_locked_queue(|_| {
                    let opts = host.opts();
                    if opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE {
                        let f = host.file_snap(file_ptr);
                        let n = host.nzb_snap(f.nzb_ptr, false);
                        let arts = snapshot_file_articles(host, file_ptr, f.articles_len);
                        tup = (
                            f.output_filename,
                            n.dest_dir,
                            f.filename,
                            arts,
                            opts.dupe_article_fallback,
                        );
                    }
                });
                tup
            };

            if daf_mode != crate::queuesubsys_host::DAF_NONE
                && (path.is_empty() || !crate::fsops::file_exists(&path, &crate::fsops::posix_stat))
            {
                path = join_path(&dest_dir, &filename);
            }
            let wrong_indices = if daf_mode != crate::queuesubsys_host::DAF_NONE {
                crate::dupefallback::borrowed_par2_mismatches_slice(
                    &articles, &path, &dest_dir, &filename,
                )
            } else {
                Vec::new()
            };

            host.with_locked_queue(|dq_ptr| {
                if !wrong_indices.is_empty() {
                    let mut f = host.file_snap(file_ptr);
                    let mut n = host.nzb_snap(f.nzb_ptr, false);
                    let mut fresh_arts = snapshot_file_articles(host, file_ptr, f.articles_len);
                    self.reject_borrowed_articles(
                        host,
                        &mut n,
                        &mut f,
                        &mut fresh_arts,
                        &wrong_indices,
                    );
                }
                self.delete_downloader(host, dq_ptr, ad_ptr, true);
            });
        }
        self.drain_live_par_completed_files(host);
        Self::drain_live_par_articles(host);
    }

    fn reject_borrowed_articles(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        articles: &mut [crate::queuesubsys_host::ArticleSnap],
        wrong_indices: &[usize],
    ) {
        let mut donor_ids = Vec::with_capacity(wrong_indices.len());
        let mut rejected_count = 0usize;
        for &idx in wrong_indices {
            if idx >= articles.len() {
                continue;
            }
            rejected_count += 1;
            donor_ids.push(articles[idx].dupe_donor_id);
            if articles[idx].status == crate::queuesubsys_host::AI_FINISHED {
                self.count_as_failed(host, nzb, file, &mut articles[idx]);
            }
        }
        let (unique_ids, formatted_names) = format_donor_id_list(&donor_ids);
        for id in unique_ids {
            host.nzb_block_donor(nzb.ptr, id);
        }
        let mut msg = format!("{} article(s) of ", rejected_count).into_bytes();
        msg.extend_from_slice(&file.filename);
        msg.extend_from_slice(
            b" borrowed from duplicates don't match its par2 checksums: counted as failed, and duplicate(s) ",
        );
        msg.extend_from_slice(&formatted_names);
        msg.extend_from_slice(b" lend this download no more articles");
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
    }

    fn delete_downloader(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        ad_ptr: usize,
        file_completed: bool,
    ) {
        let ad = host.ad_snap(ad_ptr);
        let file = host.file_snap(ad.file_ptr);
        let nzb = host.nzb_snap(file.nzb_ptr, false);
        let delete_file_obj = file_completed || (file.deleted && file.active_downloads == 1);

        host.remove_active_download(ad_ptr);

        let next_f_ad = file.active_downloads - 1;
        let next_n_ad = nzb.active_downloads - 1;
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_ACTIVE_DOWNLOADS,
            next_f_ad,
        );
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_ACTIVE_DOWNLOADS,
            next_n_ad,
        );

        if delete_file_obj {
            let nzb_id = nzb.id;
            self.delete_file_info(host, dq_ptr, file.ptr, file_completed);
            let found_nzb = host.queue_find_by_id(dq_ptr, nzb_id);
            if found_nzb.is_some() {
                host.nzb_set_int(found_nzb, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
                host.dq_save_changed(dq_ptr);
            }
        } else if next_n_ad == 0 {
            host.prepost_check_download_completed(dq_ptr, nzb.ptr);
        }
    }

    fn delete_file_info(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        file_ptr: FileRef,
        completed: bool,
    ) {
        while host.article_cache_file_busy(file_ptr) {
            host.sleep_ms(5);
        }

        let opts = host.opts();
        let file = host.file_snap(file_ptr);
        let nzb = host.nzb_snap(file.nzb_ptr, false);
        let parking = nzb.parking;
        let file_deleted = file.deleted;

        host.file_set_int(file.ptr, crate::queuesubsys_host::FILE_INT_DELETED, 1);

        if completed && file.dupe_attempted_articles > 0 {
            let mut msg = format!(
                "Recovered {} of {} missing article(s) of ",
                file.dupe_recovered_articles, file.dupe_attempted_articles
            )
            .into_bytes();
            msg.extend_from_slice(&file.filename);
            msg.extend_from_slice(b" from duplicate collections");
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
        }

        if completed || nzb.deleting {
            host.nzb_update_completed_stats(nzb.ptr, file.ptr);
        } else {
            host.nzb_update_deleted_stats(nzb.ptr, file.ptr);
        }

        let file_status = classify_completed_file_status(
            file.total_articles,
            file.success_articles,
            file.missed_articles,
            file.failed_articles,
        );

        if opts.server_mode {
            let (del_data, save_part) = eval_delete_file_diskstate_flags(
                file_status,
                crate::queuesubsys_host::CF_SUCCESS,
                crate::queuesubsys_host::CF_PARTIAL,
                file_deleted as i32,
                parking as i32,
                completed as i32,
            );
            host.disk_discard_file(file.id, del_data, true, false);
            if save_part {
                host.disk_save_file_state(file.ptr, true);
            }
        }

        if !completed {
            self.discard_temp_files(host, &opts, &file);
        }

        if completed || parking {
            let out_exists = !file.output_filename.is_empty()
                && crate::fsops::file_exists(&file.output_filename, &crate::fsops::posix_stat);
            let filename = resolve_completed_filename(
                completed,
                file_status,
                &file.output_filename,
                &file.filename,
                &nzb.dest_dir,
                crate::paths::PATH_SEPARATOR,
                out_exists,
            );

            let articles = snapshot_file_articles(host, file.ptr, file.articles_len);
            let fresh_nzb = host.nzb_snap(nzb.ptr, false);

            if completed {
                crate::dupefallback::par_damage_file_completed_host(
                    host, &fresh_nzb, &file, &articles, &filename,
                );
                if live_par_syndromes(&opts) {
                    if let Err(error) = Self::capture_live_completed_file(
                        &fresh_nzb,
                        &file,
                        &articles,
                        &filename,
                        file_status == crate::queuesubsys_host::CF_SUCCESS,
                    ) {
                        let message = Self::live_par_deferred_message(&error).replacen(
                            "catch-up",
                            "completion",
                            1,
                        );
                        host.nzb_print_message(
                            nzb.ptr,
                            crate::queuesubsys_host::MK_DETAIL,
                            message.as_bytes(),
                        );
                    }
                }
            }

            let is_incomplete = completed
                && (file_status == crate::queuesubsys_host::CF_PARTIAL
                    || file_status == crate::queuesubsys_host::CF_FAILURE);
            let is_side = is_incomplete && is_side_file(&filename, &opts.ext_cleanup_disk);
            if is_side {
                let mut msg = b"Not repairing side file ".to_vec();
                msg.extend_from_slice(&filename);
                msg.extend_from_slice(b" from duplicates");
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
            }
            if is_incomplete
                && !is_side
                && crate::dupefallback::build_repair_job_host(
                    host, &opts, &fresh_nzb, &file, &articles, &filename,
                )
            {
                let mut msg = b"Queueing stream repair of ".to_vec();
                msg.extend_from_slice(&filename);
                if file_status == crate::queuesubsys_host::CF_FAILURE {
                    msg.extend_from_slice(b" (no article available) from duplicate collections");
                } else {
                    msg.extend_from_slice(b" from duplicate collections");
                }
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);

                let flen = host.nzb_file_len(nzb.ptr);
                let mut more_files_queued = false;
                for i in 0..flen {
                    let qf_ptr = host.nzb_file_at(nzb.ptr, i);
                    if qf_ptr != file.ptr {
                        let qf = host.file_snap(qf_ptr);
                        if !qf.deleted {
                            more_files_queued = true;
                            break;
                        }
                    }
                }
                let latest_nzb = host.nzb_snap(nzb.ptr, false);
                if opts.dupe_article_fallback == crate::queuesubsys_host::DAF_LIVE
                    && more_files_queued
                    && !latest_nzb.has_live_repair_thread
                {
                    host.stream_repair_start_live(nzb.ptr);
                }
            }

            let file_crc = if file_status == crate::queuesubsys_host::CF_SUCCESS {
                file.crc
            } else {
                0
            };
            host.nzb_push_completed_file(nzb.ptr, file.ptr, &filename, file_status, file_crc);

            // A dead volume the live repair could not place yet may be placeable now that
            // another volume arrived, and a volume that just went dead wants a borrow pass of
            // its own: run it again while direct unpack waits on one.
            if completed
                && opts.dupe_article_fallback == crate::queuesubsys_host::DAF_LIVE
                && opts.direct_unpack
                && crate::postproc::direct_unpack_waits_on_dead_volume(nzb.id)
            {
                if host.nzb_snap(nzb.ptr, false).has_live_repair_thread {
                    crate::postproc::request_dead_volume_rerun(nzb.id);
                } else {
                    host.stream_repair_start_live(nzb.ptr);
                }
            }

            // Live par2: fetch the recovery volumes for the damage known so far. Checked again
            // when a par2 file completes, because the damage may predate the par2 index.
            // Both follow the repair settings: nothing is fetched or repaired on its own with
            // ParRepair=no or ParCheck=manual.
            let auto_par_repair = automatic_live_par_repair(&opts);
            if completed
                && !is_side
                && (is_incomplete || file.par_file)
                && auto_par_repair
                && crate::pardamage::has_damage(nzb.id)
            {
                let latest_nzb = host.nzb_snap(nzb.ptr, false);
                if crate::dupefallback::unpause_pars_for_damage_host(host, &latest_nzb) > 0 {
                    host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
                }
            }

            // Live par2 cutover for a paused direct unpack: repair the held volumes as soon as
            // the recovery blocks on disk cover all damaged and not yet downloaded slices.
            if completed && auto_par_repair && crate::postproc::direct_unpack_has_holds(nzb.id) {
                if let Some(st) = crate::parchecker::streaming_session_status(nzb.id) {
                    let snapshot = crate::liveparlog::Snapshot {
                        verified: st.verified,
                        damaged: st.damaged,
                        not_arrived: st.unarrived + st.partially_arrived,
                        total: st.total,
                        recovery_ready: st.ready_recovery,
                        recovery_in_flight: st.unpaused_in_flight,
                        recovery_needed: st.needed_recovery_blocks,
                        repair_possible: st.early_cutover_possible,
                    };
                    if let Some(tail) = crate::liveparlog::line_if_due(nzb.id, snapshot) {
                        let mut msg = b"Live par2 for ".to_vec();
                        msg.extend_from_slice(&nzb.name);
                        msg.extend_from_slice(tail.as_bytes());
                        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
                    }
                }
                crate::postproc::direct_unpack_try_live_par_repair(
                    host,
                    &host.nzb_snap(nzb.ptr, false),
                );
            }
        }

        if opts.direct_rename {
            crate::directrenamer::file_downloaded_host(self, host, &opts, dq_ptr, file.ptr);
        }

        let check_nzb = host.nzb_snap(nzb.ptr, false);
        if check_nzb.direct_rename_status == DirectRenameStatus::Running
            && !check_nzb.deleting
            && check_nzb.is_download_completed
        {
            self.discard_direct_rename(host, &opts, nzb.ptr);
        }

        let removed_action = if completed && !file_deleted {
            crate::queuesubsys_host::ASPECT_FILE_COMPLETED
        } else {
            crate::queuesubsys_host::ASPECT_FILE_DELETED
        };
        host.nzb_remove_file_and_notify(dq_ptr, nzb.ptr, file.ptr, removed_action);
    }

    fn demote_misaligned_dupe_neighbors(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        articles_len: usize,
        index: usize,
    ) {
        let mut neighbors = [None, None];
        if index > 0 {
            neighbors[0] = Some(index - 1);
        }
        if index + 1 < articles_len {
            neighbors[1] = Some(index + 1);
        }
        for nb_opt in neighbors {
            let Some(nb_idx) = nb_opt else {
                continue;
            };
            let mut nb = host.article_at(file.ptr, nb_idx);
            if nb.status != crate::queuesubsys_host::AI_FINISHED
                || nb.dupe_fallback_round <= 0
                || crate::dupefallback::segment_aligned_at(
                    host,
                    file.ptr,
                    articles_len,
                    nb_idx,
                    &nb,
                    file.decoded_file_size,
                )
            {
                continue;
            }
            let part_num = nb.part_number;
            self.demote_finished_article(host, nzb, file, &mut nb);
            let mut msg = b"Discarding misaligned article ".to_vec();
            msg.extend_from_slice(&file.filename);
            msg.extend_from_slice(
                format!(" [{}/{}] from duplicate", part_num, articles_len).as_bytes(),
            );
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
        }
    }

    fn validate_completed_file_tiling(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        articles: &mut [crate::queuesubsys_host::ArticleSnap],
    ) {
        if file.success_articles != file.total_articles {
            return;
        }
        if !crate::dupefallback::has_donor_articles_slice(articles) {
            return;
        }
        let Some(untiled_idx) =
            crate::dupefallback::first_untiled_article_slice(articles, file.decoded_file_size)
        else {
            return;
        };
        let part_num = articles[untiled_idx].part_number;
        let total_len = articles.len();
        self.demote_finished_article(host, nzb, file, &mut articles[untiled_idx]);
        let mut msg = b"Discarding misaligned article ".to_vec();
        msg.extend_from_slice(&file.filename);
        msg.extend_from_slice(
            format!(
                " [{}/{}]: decoded bytes do not tile the file (leaving to par-repair)",
                part_num, total_len
            )
            .as_bytes(),
        );
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
    }

    fn count_as_failed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        art: &mut crate::queuesubsys_host::ArticleSnap,
    ) {
        // Every accepted-to-failed route must revoke source identity, including
        // direct checksum demotion that does not discard cached content here.
        host.invalidate_source_replacement(file.ptr, art.ptr);
        art.status = crate::queuesubsys_host::AI_FAILED;
        host.article_edit(
            art.ptr,
            crate::qarticle::ArticleEdit::Status(crate::queuesubsys_host::AI_FAILED),
        );
        let par_recovered_before = file.dupe_recovered_articles;
        demote_article_counters(
            art.size,
            file.par_file,
            art.dupe_proactive,
            &art.message_id,
            &art.dupe_original_message_id,
            &mut file.success_size,
            &mut file.failed_size,
            &mut file.success_articles,
            &mut file.failed_articles,
            &mut nzb.current_success_size,
            &mut nzb.current_failed_size,
            &mut nzb.par_current_success_size,
            &mut nzb.par_current_failed_size,
            &mut nzb.current_success_articles,
            &mut nzb.current_failed_articles,
            &mut file.dupe_recovered_articles,
            &mut nzb.dupe_recovered_articles,
        );
        // a borrowed par2 article taken back: it no longer counts as borrowed
        if file.par_file && file.dupe_recovered_articles < par_recovered_before {
            note_dupe_par_article(host, nzb.ptr, file.id, false);
        }
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_SUCCESS_ARTICLES,
            file.success_articles,
        );
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_FAILED_ARTICLES,
            file.failed_articles,
        );
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_CUR_SUCCESS_ARTICLES,
            nzb.current_success_articles,
        );
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_CUR_FAILED_ARTICLES,
            nzb.current_failed_articles,
        );
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_DUPE_RECOVERED_ARTICLES,
            file.dupe_recovered_articles,
        );
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DUPE_RECOVERED_ARTICLES,
            nzb.dupe_recovered_articles,
        );
        host.file_set_i64(
            file.ptr,
            crate::queuesubsys_host::FILE_I64_SUCCESS_SIZE,
            file.success_size,
        );
        host.file_set_i64(
            file.ptr,
            crate::queuesubsys_host::FILE_I64_FAILED_SIZE,
            file.failed_size,
        );
        host.nzb_set_i64(
            nzb.ptr,
            crate::queuesubsys_host::NZB_I64_CUR_SUCCESS_SIZE,
            nzb.current_success_size,
        );
        host.nzb_set_i64(
            nzb.ptr,
            crate::queuesubsys_host::NZB_I64_CUR_FAILED_SIZE,
            nzb.current_failed_size,
        );
        host.nzb_set_i64(
            nzb.ptr,
            crate::queuesubsys_host::NZB_I64_PAR_CUR_SUCCESS_SIZE,
            nzb.par_current_success_size,
        );
        host.nzb_set_i64(
            nzb.ptr,
            crate::queuesubsys_host::NZB_I64_PAR_CUR_FAILED_SIZE,
            nzb.par_current_failed_size,
        );
    }

    fn demote_finished_article(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        art: &mut crate::queuesubsys_host::ArticleSnap,
    ) {
        self.discard_article_segment(host, file, art);
        self.count_as_failed(host, nzb, file, art);
        if crate::dupefallback::register_lead_failure_host(host, file, art) {
            let mut msg = b"Switching lead duplicate collection for ".to_vec();
            msg.extend_from_slice(&file.filename);
            msg.extend_from_slice(b" (the lead duplicate is missing many articles)");
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
        }
        file.partial_changed = true;
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED,
            1,
        );
    }

    fn discard_article_segment(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        file: &mut crate::queuesubsys_host::FileSnap,
        art: &crate::queuesubsys_host::ArticleSnap,
    ) {
        host.invalidate_source_replacement(file.ptr, art.ptr);
        host.article_cache_discard_segment(file.ptr, art.ptr);
        file.cached_articles = host.file_snap(file.ptr).cached_articles;
    }

    fn save_queue_if_changed(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        if !host.opts().server_mode {
            return;
        }
        host.with_locked_queue(|dq_ptr| {
            let qlen = host.queue_len(dq_ptr);
            let mut has_changes = false;
            for i in 0..qlen {
                let nzb = host.nzb_snap(host.queue_at(dq_ptr, i), false);
                // Live PAR time not yet in the saved times would be lost on restart.
                has_changes |= nzb.changed || crate::parchecker::live_par_time_pending(nzb.id);
            }
            if has_changes {
                host.dq_save(dq_ptr);
            }
        });
    }

    fn save_all_partial_state(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        let opts = host.opts();
        if !opts.server_mode || !opts.continue_partial {
            return;
        }
        host.with_locked_queue(|dq_ptr| {
            let qlen = host.queue_len(dq_ptr);
            for i in 0..qlen {
                let nzb_ptr = host.queue_at(dq_ptr, i);
                let flen = host.nzb_file_len(nzb_ptr);
                for j in 0..flen {
                    self.save_partial_state(host, host.nzb_file_at(nzb_ptr, j));
                }
            }
            host.dq_save_changed(dq_ptr);
        });
    }

    fn save_partial_state(&self, host: &crate::queuesubsys_host::QueueSubsysHost, file_ptr: FileRef) {
        save_partial_state_host(host, file_ptr);
    }

    /// Loads a file's article list (and partial state) from disk unless it
    /// is already in memory, and returns a fresh snapshot. Always decides on
    /// a fresh read: the loader appends, so loading twice doubles the list
    /// (137 articles became 274 on a restart), the saved state then no longer
    /// matches it and the file is written under its temporary name.
    fn ensure_articles_loaded(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb: &crate::queuesubsys_host::NzbSnap,
        file_ptr: FileRef,
    ) -> crate::queuesubsys_host::FileSnap {
        let file = host.file_snap(file_ptr);
        if file.articles_len > 0 || !opts.server_mode {
            return file;
        }
        host.disk_load_articles(file_ptr);
        self.load_partial_state(host, opts, nzb, file_ptr);
        host.file_snap(file_ptr)
    }

    fn load_partial_state(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb: &crate::queuesubsys_host::NzbSnap,
        file_ptr: FileRef,
    ) {
        let file = host.file_snap(file_ptr);
        if file.partial_state == crate::queuesubsys_host::PARTIAL_PARTIAL {
            host.disk_load_file_state(file.ptr, false);
            let f2 = host.file_snap(file.ptr);
            if (opts.direct_write || f2.force_direct_write) && !f2.output_initialized {
                let out_tmp = join_path(&nzb.dest_dir, format!("{}.out.tmp", f2.id).as_bytes());
                if crate::fsops::file_exists(&out_tmp, &crate::fsops::posix_stat) {
                    host.file_set_str(
                        file.ptr,
                        crate::queuesubsys_host::FILE_STR_OUTPUT_FILENAME,
                        Some(&out_tmp),
                    );
                    host.file_set_int(
                        file.ptr,
                        crate::queuesubsys_host::FILE_INT_OUTPUT_INITIALIZED,
                        1,
                    );
                } else if f2.force_direct_write {
                    let out_fn = join_path(&nzb.dest_dir, &f2.filename);
                    if crate::fsops::file_exists(&out_fn, &crate::fsops::posix_stat) {
                        host.file_set_str(
                            file.ptr,
                            crate::queuesubsys_host::FILE_STR_OUTPUT_FILENAME,
                            Some(&out_fn),
                        );
                        host.file_set_int(
                            file.ptr,
                            crate::queuesubsys_host::FILE_INT_OUTPUT_INITIALIZED,
                            1,
                        );
                    }
                }
            }
        } else if file.partial_state == crate::queuesubsys_host::PARTIAL_COMPLETED {
            host.disk_load_file_state(file.ptr, true);
            let f2 = host.file_snap(file.ptr);
            let out_fn = join_path(&nzb.dest_dir, &f2.filename);
            host.file_set_str(
                file.ptr,
                crate::queuesubsys_host::FILE_STR_OUTPUT_FILENAME,
                Some(&out_fn),
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_OUTPUT_INITIALIZED,
                1,
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_FORCE_DIRECT_WRITE,
                1,
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_FILENAME_CONFIRMED,
                1,
            );
        }
    }

    fn save_all_file_state(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        if host.opts().server_mode && host.coord_dq_state_changed() {
            host.with_locked_queue(|dq_ptr| {
                host.disk_save_all_file_infos(dq_ptr);
            });
        }
    }

    fn check_health(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        file_ptr: FileRef,
    ) {
        let file = host.file_snap(file_ptr);
        let nzb = host.nzb_snap(file.nzb_ptr, false);
        let dl_crit = self.download_critical_health(host, opts, dq_ptr, &nzb);
        if should_skip_health_check(
            opts.health_check,
            crate::queuesubsys_host::HC_NONE,
            crate::queuesubsys_host::HC_PARK,
            opts.par_scan,
            crate::queuesubsys_host::PS_DUPE,
            nzb.health_paused,
            nzb.delete_status,
            nzb.health,
            dl_crit,
            nzb.success_articles,
            nzb.total_articles,
        ) {
            return;
        }

        if opts.health_check == crate::queuesubsys_host::HC_PAUSE {
            let mut msg = b"Pausing ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(
                format!(
                    " due to health {:.1}% below critical {:.1}%",
                    (nzb.health as f64) / 10.0,
                    (nzb.critical_health_opt as f64) / 10.0
                )
                .as_bytes(),
            );
            host.log(crate::queuesubsys_host::MK_WARNING, &msg);
            host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_HEALTH_PAUSED, 1);
            host.dq_edit_entry(
                dq_ptr,
                nzb.id,
                crate::queuesubsys_host::EA_GROUP_PAUSE,
                None,
            );
        } else if opts.health_check == crate::queuesubsys_host::HC_DELETE
            || opts.health_check == crate::queuesubsys_host::HC_PARK
        {
            let mut msg = b"Cancelling download and deleting ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(
                format!(
                    " due to health {:.1}% below critical {:.1}%",
                    (nzb.health as f64) / 10.0,
                    (nzb.critical_health_opt as f64) / 10.0
                )
                .as_bytes(),
            );
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
            if opts.health_check == crate::queuesubsys_host::HC_PARK {
                crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
                return;
            }
            host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Health);
            host.dq_edit_entry(
                dq_ptr,
                nzb.id,
                if opts.health_check == crate::queuesubsys_host::HC_PARK {
                    crate::queuesubsys_host::EA_GROUP_PARK_DELETE
                } else {
                    crate::queuesubsys_host::EA_GROUP_DELETE
                },
                None,
            );
        } else if opts.health_check == crate::queuesubsys_host::HC_DUPE {
            self.check_dupe_failover(host, opts, dq_ptr, &nzb);
        }
    }

    fn download_critical_health(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> i32 {
        if opts.health_check == crate::queuesubsys_host::HC_DUPE
            && nzb.critical_health_exact == 1000
            && Self::certainly_parless(host, nzb)
            && !Self::has_size_twin(host, dq_ptr, nzb)
        {
            return 1000;
        }
        nzb.critical_health_opt
    }

    fn certainly_parless(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> bool {
        if crate::dupefallback::has_par2_host(host, nzb) {
            return false;
        }
        let mut files = 0i32;
        let flen = host.nzb_file_len(nzb.ptr);
        for i in 0..flen {
            let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
            if !is_data_filename(&f.filename) {
                return false;
            }
            files += 1;
        }
        for cf in host.nzb_completed_files(nzb.ptr) {
            if !is_data_filename(&cf.filename) {
                return false;
            }
            files += 1;
        }
        files > 0
    }

    fn has_size_twin(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> bool {
        let mut largest = 0i64;
        let flen = host.nzb_file_len(nzb.ptr);
        for i in 0..flen {
            let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
            largest = largest.max(f.size);
        }
        if largest <= 0 {
            return false;
        }
        let check_other = |other_ptr: NzbRef| -> bool {
            if other_ptr.is_none() || other_ptr == nzb.ptr {
                return false;
            }
            let other = host.nzb_snap(other_ptr, false);
            let same_key = crate::dupecoord::same_name_or_key(
                &other.name,
                &other.dupe_key,
                &nzb.name,
                &nzb.dupe_key,
            );
            let oflen = host.nzb_file_len(other_ptr);
            let mut sizes = Vec::with_capacity(oflen);
            for j in 0..oflen {
                sizes.push(host.file_snap(host.nzb_file_at(other_ptr, j)).size);
            }
            is_size_twin(
                other_ptr == nzb.ptr,
                same_key,
                nzb.size,
                other.size,
                other.par_size,
                largest,
                &sizes,
            )
        };
        let qlen = host.queue_len(dq_ptr);
        for i in 0..qlen {
            if check_other(host.queue_at(dq_ptr, i)) {
                return true;
            }
        }
        let hlen = host.history_len(dq_ptr);
        for i in 0..hlen {
            let hi = host.history_at(dq_ptr, i);
            if hi.kind == crate::queuesubsys_host::HK_NZB && check_other(hi.nzb()) {
                return true;
            }
        }
        false
    }

    fn has_active_dupe(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> bool {
        if nzb.dupe_key.is_empty() {
            return false;
        }
        if nzb.has_extra_priority
            || (nzb.has_post_info
                && (nzb.post_working || nzb.post_stage != PostStage::Queued))
        {
            return false;
        }
        let self_completed_empty = host.nzb_completed_files(nzb.ptr).is_empty();
        let qlen = host.queue_len(dq_ptr);
        for i in 0..qlen {
            let qp = host.queue_at(dq_ptr, i);
            if qp.is_none() || qp == nzb.ptr {
                continue;
            }
            let queued = host.nzb_snap(qp, false);
            if !queued.dupe_key.is_empty() && queued.dupe_key.eq_ignore_ascii_case(&nzb.dupe_key) {
                if repair_lets_other_copies_download(
                    queued.has_post_info,
                    queued.post_stage,
                    &queued.dest_dir,
                    &nzb.dest_dir,
                    || {
                        let o = host.opts();
                        (o.health_check, o.inter_dir)
                    },
                ) {
                    continue;
                }
                if is_queued_dupe_active(
                    queued.deleting,
                    queued.delete_status,
                    queued.active_downloads,
                    queued.has_post_info,
                    queued.post_working,
                    queued.post_stage,
                    queued.has_extra_priority,
                    self_completed_empty,
                    queued.direct_unpack_status,
                ) {
                    return true;
                }
            }
        }
        false
    }

    fn check_dupe_failover(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) {
        if nzb.deleting || nzb.parking {
            return;
        }
        let attempted = nzb.dupe_attempted_articles + nzb.dupe_unsourced_articles;
        let recovered = nzb.dupe_recovered_articles;
        let failed = nzb.current_failed_articles;
        let (defer, upd_checked) = should_defer_dupe_failover(
            opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE,
            attempted,
            recovered,
            DUPE_FAILOVER_SAMPLE,
            nzb.dupe_par_defer_state,
            crate::queuesubsys_host::DP_DEFERRED,
            crate::dupefallback::par_cannot_cover_snap(nzb, true),
            failed,
            nzb.dupe_failover_checked,
        );
        if defer {
            return;
        }
        if upd_checked {
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DUPE_FAILOVER_CHECKED,
                failed,
            );
        }

        let backup = if crate::dupecoord::fails_over_snap(opts, nzb) {
            crate::dupecoord::find_dupe_backup_host(
                host,
                opts,
                dq_ptr,
                nzb.ptr,
                &nzb.name,
                &nzb.dupe_key,
            )
            .filter(|(_, b)| !crate::dupecoord::locked_host(host, b.ptr))
        } else {
            None
        };

        let dl_crit = self.download_critical_health(host, opts, dq_ptr, nzb);
        let effective_failover_health = if dl_crit > nzb.critical_health_opt {
            0
        } else {
            nzb.health
        };

        // Option DupeBorrowFirst: light damage is repaired from the duplicates (live stream
        // repair, then post-processing) instead of replacing the download. Only a download
        // that is failing in bulk is replaced while it downloads.
        let light_damage = crate::options::dupe_borrow_first()
            && opts.dupe_article_fallback >= crate::queuesubsys_host::DAF_STREAM
            && !dupe_mass_failure(nzb.total_articles, failed);

        // Set when the backup feeds the running extraction: neither a failover nor parking the
        // download as hopeless may throw that extraction away.
        let mut feeds_extraction = false;
        if let Some((backup_hi, backup_nzb)) = backup {
            // A backup that only just arrived has not been classified yet: give it the
            // grace to be sampled, so a twin or alt that can lend is not skipped over by
            // a failover that throws away the extraction that runs.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            let backup_kind = crate::donorkind::DonorKind::from_param(
                host.nzb_get_param(backup_nzb.ptr, crate::donorkind::KIND_PARAM)
                    .as_deref(),
            );
            if backup_still_unclassified(
                backup_kind,
                backup_twincheck_labelled(host, backup_nzb.ptr),
                backup_hi.time,
                now,
                crate::dupesearch_coord::intake_pending(backup_nzb.id, &nzb.dupe_key),
                extraction_fed_by_duplicates(opts),
            ) && !download_hopeless(nzb.total_articles, nzb.current_success_articles, failed)
            {
                // Not checked yet: the next round looks again instead of waiting for
                // another sample of failed articles.
                if upd_checked {
                    host.nzb_set_int(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_INT_DUPE_FAILOVER_CHECKED,
                        nzb.dupe_failover_checked,
                    );
                }
                return;
            }
            // A stretch of lost articles (whole volumes included) that a twin or a
            // borrowable alt can lend is borrowed too, keeping the extraction that runs.
            let lendable =
                alt_covers_lost_articles(
                    false,
                    live_borrowing(opts),
                    crate::donorkind::DonorKind::from_param(
                        host.nzb_get_param(nzb.ptr, crate::donorkind::KIND_PARAM)
                            .as_deref(),
                    ),
                    backup_kind,
                    nzb.health,
                    crate::postproc::dead_volume_borrow_underway(nzb.id),
                    nzb.direct_unpack_status == DirectUnpackStatus::Running,
                    backup_may_lend_while_extracting(host, backup_nzb.ptr),
                    backup_cannot_lend(host, nzb, &backup_nzb),
                ) && (nzb.direct_unpack_status == DirectUnpackStatus::Running
                    || !download_hopeless(
                        nzb.total_articles,
                        nzb.current_success_articles,
                        failed,
                    ));
            // A primary cut at its frontier lends from its alt by design (`frontiercut`).
            let lendable = lendable
                || (crate::frontiercut::is_cut(nzb.id)
                    && backup_continues_extraction(host, &opts, &nzb, &backup_nzb));
            // A duplicate whose bytes the running extraction already patched in (a nested or
            // otherwise unlabelled alt the cross-packing repair maps by the video inside) lends:
            // the article-level count of recovered articles misses those bytes.
            let lendable = lendable
                || (nzb.direct_unpack_status == DirectUnpackStatus::Running
                    && nzb.dupe_recovered_bytes > 0
                    && !backup_cannot_lend(host, nzb, &backup_nzb));
            // An extraction direct unpack already finished and verified has nothing left to
            // lose or gain: replacing the download only downloads and extracts it again (nest3v
            // on 6790 failed over 3 s after its verified extraction).
            let extraction_verified = direct_unpack_verified(nzb.direct_unpack_status);
            let lendable = lendable || extraction_verified;
            feeds_extraction = lendable
                && (nzb.direct_unpack_status == DirectUnpackStatus::Running || extraction_verified);
            if !light_damage
                && !lendable
                && crate::dupecoord::dupe_failover_warranted_inline(
                    nzb.dupe_score,
                    effective_failover_health,
                    backup_nzb.dupe_score,
                )
            {
                note_cannot_continue(host, &opts, &nzb, &backup_nzb);
                let mut msg = b"Failing over ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" to duplicate ");
                msg.extend_from_slice(&backup_nzb.name);
                msg.extend_from_slice(
                    format!(
                        ": health {:.1}% below critical {:.1}%, {} of {} missing article(s) recovered from duplicates",
                        (nzb.health as f64) / 10.0,
                        (dl_crit as f64) / 10.0,
                        recovered,
                        attempted
                    )
                    .as_bytes(),
                );
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
                set_projected_from_tried(host, nzb);
                crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
                return;
            }
        }

        if !feeds_extraction
            && download_hopeless(
                nzb.total_articles,
                nzb.current_success_articles,
                nzb.current_failed_articles,
            )
        {
            let tried = nzb.current_success_articles + nzb.current_failed_articles;
            set_projected_from_tried(host, nzb);
            let reason = crate::dupecoord::no_backup_reason_host(host, opts, dq_ptr, nzb);
            let mut msg = b"Parking ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(
                format!(
                    ": health {:.1}% below critical {:.1}%, {} of {} missing article(s) recovered from duplicates, {} of {} tried article(s) exist and no better duplicate in history (",
                    (nzb.health as f64) / 10.0,
                    (dl_crit as f64) / 10.0,
                    recovered,
                    attempted,
                    nzb.current_success_articles,
                    tried
                )
                .as_bytes(),
            );
            msg.extend_from_slice(&reason);
            msg.push(b')');
            crate::dupecoord::push_queued_copies_note(host, dq_ptr, nzb, &mut msg);
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
            crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
        }
    }

    fn check_dead_download(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
    ) {
        let nzb = host.nzb_snap(nzb_ptr, false);
        if opts.health_check != crate::queuesubsys_host::HC_DUPE
            || !opts.dupe_check
            || !crate::dupecoord::fails_over_snap(opts, &nzb)
            || dead_check_skipped(&nzb)
        {
            return;
        }

        // Kept to borrow its dead volumes from a backup whose video then turned out to differ:
        // nothing else would fail it over before its end (its later volumes arrive whole), so
        // it fails over now.
        if was_kept_for_borrowing(nzb.id) {
            if let Some((_, backup_nzb)) = crate::dupecoord::find_dupe_backup_host(
                host,
                opts,
                dq_ptr,
                nzb.ptr,
                &nzb.name,
                &nzb.dupe_key,
            ) {
                if backup_cannot_lend(host, &nzb, &backup_nzb)
                    && !crate::dupecoord::locked_host(host, backup_nzb.ptr)
                {
                    if let Some(g) = KEPT_FOR_BORROWING
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_mut()
                    {
                        g.remove(&nzb.id);
                    }
                    note_cannot_continue(host, &opts, &nzb, &backup_nzb);
                    let mut msg = b"Failing over ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    msg.extend_from_slice(b" to duplicate ");
                    msg.extend_from_slice(&backup_nzb.name);
                    msg.extend_from_slice(
                        b": it was kept to borrow its dead volumes, but the duplicate holds another video",
                    );
                    host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
                    crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
                    return;
                }
            }
        }

        let critical = self.download_critical_health(host, opts, dq_ptr, &nzb);
        let probing = crate::dupeprobe::probe_probing_for(nzb.id);
        let failed = nzb.current_failed_articles;
        let files_tried = Self::files_tried(host, &nzb);
        // Judge the data files only: par2 recovery files are tried early (live PAR asks for
        // them), and a dead one says nothing about whether the data can be finished, from the
        // duplicates or otherwise. Their articles are taken out by their share of the size.
        let eval = eval_dead_download(
            data_articles(
                nzb.current_success_articles,
                nzb.current_success_size,
                nzb.par_current_success_size,
            ),
            nzb.dupe_recovered_articles,
            data_articles(failed, nzb.current_failed_size, nzb.par_current_failed_size),
            nzb.file_count,
            nzb.dupe_failed_run,
            crate::dupefallback::par_cannot_cover_snap(&nzb, false),
            critical,
            crate::dupefallback::PAR_EDGE_MARGIN,
            files_tried,
            probing,
        );
        let dead = eval.dead != 0;
        let doomed = eval.doomed != 0;
        let hopeless = eval.hopeless != 0;
        let dead_rate = eval.dead_rate != 0;
        let far_below = eval.far_below != 0;
        let below = eval.below != 0;

        // A doomed download (projected health near critical) still gets the par2 check: the
        // doomed failover below needs a backup known alive enough and may decline (functional
        // `pardamageborrow`, projected 84.5-85% against critical 81% plus the margin, missed its
        // par failover). Not while it is kept to borrow its lost parts, a dead-volume borrow
        // runs, or its backup is still within the classify grace: those decide first below.
        if !dead
            && (!doomed
                || self.doomed_par_check_allowed(
                    host,
                    opts,
                    dq_ptr,
                    &nzb,
                    dead_rate,
                    eval.tried,
                    eval.projected,
                ))
            && self.check_par_damage(host, opts, dq_ptr, &nzb)
        {
            return;
        }
        if (!dead && !doomed) || (probing >= 0 && !hopeless) {
            return;
        }
        // Judged on the articles tried so far: the health shown is the projection, not
        // the download's own health, which counts every untried article as good.
        if eval.tried > 0 {
            host.nzb_set_param(
                nzb.ptr,
                crate::dupecoord::PROJECTED_PARAM,
                Some(format!("{:.1}", f64::from(eval.projected.clamp(0, 1000)) / 10.0).as_bytes()),
            );
        }

        // A copy whose archive needs a password nobody has can never be extracted: failing over
        // to it would throw this download away for nothing. The selector only picks a locked copy
        // when nothing better qualifies, so keep downloading this one rather than parking it,
        // since parking would hand the slot to that same locked copy.
        let backup = crate::dupecoord::find_dupe_backup_host(
            host,
            opts,
            dq_ptr,
            nzb.ptr,
            &nzb.name,
            &nzb.dupe_key,
        );
        if backup
            .as_ref()
            .is_some_and(|(_, b)| crate::dupecoord::locked_host(host, b.ptr))
        {
            return;
        }
        let Some((backup_hi, backup_nzb)) = backup else {
            let attempted = nzb.dupe_attempted_articles + nzb.dupe_unsourced_articles;
            if should_park_dead_without_backup(
                opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE,
                nzb.dupe_par_defer_state,
                crate::queuesubsys_host::DP_DEFERRED,
                attempted,
                nzb.dupe_recovered_articles,
                eval.tried,
                PROJECTED_FAILURE_SAMPLE,
                files_tried,
                far_below,
                below,
            ) {
                let reason = crate::dupecoord::no_backup_reason_host(host, opts, dq_ptr, &nzb);
                let mut msg = b"Parking ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(
                    format!(
                        ": {} of {} tried article(s) of its own arrived, projected health {:.1}% {} critical {:.1}%, and no duplicate in history to fail over to (",
                        eval.own,
                        eval.tried,
                        (eval.projected as f64) / 10.0,
                        if far_below { "under half the" } else { "below" },
                        (critical as f64) / 10.0
                    )
                    .as_bytes(),
                );
                msg.extend_from_slice(&reason);
                msg.push(b')');
                crate::dupecoord::push_queued_copies_note(host, dq_ptr, &nzb, &mut msg);
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
                crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
            }
            return;
        };

        let backup_kind = crate::donorkind::DonorKind::from_param(
            host.nzb_get_param(backup_nzb.ptr, crate::donorkind::KIND_PARAM)
                .as_deref(),
        );
        let own_kind = crate::donorkind::DonorKind::from_param(
            host.nzb_get_param(nzb.ptr, crate::donorkind::KIND_PARAM)
                .as_deref(),
        );
        let facts = weak_switch_facts(host, opts, &nzb, &backup_nzb, eval.tried, eval.projected);
        // A primary cut at its frontier keeps its extraction while its alt lends the rest:
        // its own articles fail by design, so the dead-download check does not fail it over
        // unless the alt turned out unable to lend (a check found its video different, or
        // it could not be restarted).
        if facts.cut {
            if facts.continuable {
                return;
            }
            crate::frontiercut::forget(nzb.id);
        }
        match weak_primary_switch(&facts) {
            WeakSwitch::Stay => {}
            WeakSwitch::ContinueAtFrontier => {
                crate::frontiercut::cut(nzb.id, backup_nzb.id, video_set_file_ids(host, nzb.ptr));
                let mut msg = b"Continuing ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" at its frontier from alt ");
                msg.extend_from_slice(&backup_nzb.name);
                msg.extend_from_slice(
                    format!(
                        ": weak, {} of {} tried article(s) of its own arrived (projected {:.1}%), {:.1}% of it downloaded; the alt holds the same video (equal inner keys) and is {:.0}% alive, so it lends everything past the frontier and the extraction carries on (nothing downloaded is fetched again)",
                        eval.own,
                        eval.tried,
                        (eval.projected as f64) / 10.0,
                        (facts.done as f64) / 10.0,
                        (facts.backup_alive as f64) / 10.0
                    )
                    .as_bytes(),
                );
                host.nzb_print_message(
                    nzb.ptr,
                    crate::queuesubsys_host::MK_WARNING,
                    &crate::releasename::log_name(&msg),
                );
                return;
            }
            WeakSwitch::FailOver => {
                // the fleet's failover order picked this alt (no dupe score
                // check: held copies of a fleet are ranked by it already)
                note_cannot_continue(host, opts, &nzb, &backup_nzb);
                let mut msg = b"Failing over ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" to alt ");
                msg.extend_from_slice(&backup_nzb.name);
                msg.extend_from_slice(
                    format!(
                        ": weak, {} of {} tried article(s) of its own arrived (projected {:.1}%), {:.1}% of it downloaded; the alt holds the same video (equal inner keys) and is {:.0}% alive, so it downloads instead of borrowing every lost article",
                        eval.own,
                        eval.tried,
                        (eval.projected as f64) / 10.0,
                        (facts.done as f64) / 10.0,
                        (facts.backup_alive as f64) / 10.0
                    )
                    .as_bytes(),
                );
                host.nzb_print_message(
                    nzb.ptr,
                    crate::queuesubsys_host::MK_WARNING,
                    &crate::releasename::log_name(&msg),
                );
                // its direct unpack would wait for its next volume for ever and
                // keep the alt from downloading (a copy of the key that extracts
                // counts as active), and its live repair would go on borrowing
                // for a copy that no longer downloads
                if nzb.has_unpack_thread {
                    host.direct_unpack_stop(dq_ptr, nzb.ptr);
                }
                if nzb.has_live_repair_thread {
                    host.live_repair_stop(nzb.ptr);
                }
                // writer-fed live extraction (#721): the held copy writes no volume any more
                // and its stopped extraction reads none; left registered, a volume its reader
                // wanted would keep the article cache writing early for every download
                // (`livewrite::any_wanted`) until this copy finished, which it never does
                crate::livewrite::forget(nzb.id);
                crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
                return;
            }
        }
        if alt_covers_lost_articles(
            dead_rate,
            live_borrowing(opts),
            own_kind,
            backup_kind,
            eval.projected,
            crate::postproc::dead_volume_borrow_underway(nzb.id),
            nzb.direct_unpack_status == DirectUnpackStatus::Running,
            backup_may_lend_while_extracting(host, backup_nzb.ptr),
            backup_cannot_lend(host, &nzb, &backup_nzb),
        ) {
            if note_kept_for_borrowing(nzb.id) {
                let mut msg = b"Keeping ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(
                    format!(
                        ": {} of {} tried article(s) of its own arrived ({} failed in a row); duplicate ",
                        eval.own, eval.tried, nzb.dupe_failed_run
                    )
                    .as_bytes(),
                );
                msg.extend_from_slice(&backup_nzb.name);
                msg.extend_from_slice(
                    b" lends the lost parts (each borrow checks its video first), so the extraction carries on instead of starting over",
                );
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
            }
            return;
        }

        if dead {
            // a backup the fleet intake is still reading may yet feed this extraction: a
            // dead run waits for its verdict too, unless nothing of this copy can be saved
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            if !hopeless
                && crate::dupesearch_coord::intake_pending(backup_nzb.id, &nzb.dupe_key)
                && backup_still_unclassified(
                    backup_kind,
                    backup_twincheck_labelled(host, backup_nzb.ptr),
                    backup_hi.time,
                    now,
                    true,
                    extraction_fed_by_duplicates(opts),
                )
            {
                return;
            }
            if !crate::dupecoord::dupe_failover_warranted_inline(
                nzb.dupe_score,
                0,
                backup_nzb.dupe_score,
            ) {
                return;
            }
            note_cannot_continue(host, &opts, &nzb, &backup_nzb);
            let mut msg = b"Failing over ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" to duplicate ");
            msg.extend_from_slice(&backup_nzb.name);
            if dead_rate {
                msg.extend_from_slice(
                    format!(
                        ": {} of its own articles downloaded, {} failed",
                        eval.own, failed
                    )
                    .as_bytes(),
                );
            } else {
                msg.extend_from_slice(
                    format!(
                        ": {} of its articles failed in a row ({} of {} tried arrived)",
                        nzb.dupe_failed_run, eval.own, eval.tried
                    )
                    .as_bytes(),
                );
            }
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            if backup_still_unclassified(
                backup_kind,
                backup_twincheck_labelled(host, backup_nzb.ptr),
                backup_hi.time,
                now,
                crate::dupesearch_coord::intake_pending(backup_nzb.id, &nzb.dupe_key),
                extraction_fed_by_duplicates(opts),
            ) {
                return;
            }
            let backup_alive = backup_alive_permille(host, backup_nzb.ptr);
            if backup_alive < eval.projected + BACKUP_LEAD
                || backup_alive < critical
                || !crate::dupecoord::dupe_failover_warranted_inline(
                    nzb.dupe_score,
                    eval.projected,
                    backup_nzb.dupe_score,
                )
            {
                return;
            }
            note_cannot_continue(host, &opts, &nzb, &backup_nzb);
            let mut msg = b"Failing over ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" to duplicate ");
            msg.extend_from_slice(&backup_nzb.name);
            msg.extend_from_slice(
                format!(
                    ": {} of {} tried article(s) of its own arrived, projected health {:.1}% {} critical {:.1}%; the duplicate is {:.0}% alive",
                    eval.own,
                    eval.tried,
                    (eval.projected as f64) / 10.0,
                    if eval.projected < critical {
                        "below"
                    } else {
                        "too close to"
                    },
                    (critical as f64) / 10.0,
                    (backup_alive as f64) / 10.0
                )
                .as_bytes(),
            );
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
        }
        crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
    }

    /// Direct unpack found the download's archive needs a password that neither its nzb nor
    /// `UnpackPassFile` gives (encrypted headers, say): it can never be extracted, so it fails
    /// over to the best duplicate in history now rather than downloading whole first.
    fn check_locked_archive(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
    ) {
        let nzb = host.nzb_snap(nzb_ptr, false);
        // the mark is taken first, so one this download cannot act on never
        // lingers to fail it over later (a history return reuses the id)
        if !crate::postproc::take_direct_unpack_locked(nzb.id)
            || opts.health_check != crate::queuesubsys_host::HC_DUPE
            || !crate::dupecoord::fails_over_snap(opts, &nzb)
            || nzb.kind != crate::queuesubsys_host::NK_NZB
            || nzb.deleting
            || nzb.parking
            || nzb.delete_status != DeleteStatus::None
        {
            return;
        }
        let Some((_, backup_nzb)) = crate::dupecoord::find_dupe_backup_host(
            host,
            opts,
            dq_ptr,
            nzb.ptr,
            &nzb.name,
            &nzb.dupe_key,
        )
        .filter(|(_, b)| !crate::dupecoord::locked_host(host, b.ptr)) else {
            let mut msg = b"Downloading ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(
                b" on: its archive needs a password its nzb does not carry, and no duplicate in history to fail over to",
            );
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
            return;
        };
        let mut msg = b"Failing over ".to_vec();
        msg.extend_from_slice(&nzb.name);
        msg.extend_from_slice(b" to duplicate ");
        msg.extend_from_slice(&backup_nzb.name);
        msg.extend_from_slice(
            b": its archive needs a password its nzb does not carry, so it can never be extracted",
        );
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
        crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
    }

    fn check_par_damage(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> bool {
        let verdict = crate::dupefallback::par_damage_judge_host(host, nzb);
        let certain = verdict.certain();
        if !certain && !verdict.projected() {
            return false;
        }
        let attempted = nzb.dupe_attempted_articles + nzb.dupe_unsourced_articles;
        // (walks the files only while the sample is not reached) the lost articles still to
        // come, at the rate the data articles tried so far were lost
        let untried = if (DUPE_FAILOVER_SAMPLE / 2..DUPE_FAILOVER_SAMPLE).contains(&attempted) {
            expected_lost(
                Self::untried_articles(host, nzb),
                data_articles(
                    nzb.current_success_articles,
                    nzb.current_success_size,
                    nzb.par_current_success_size,
                ),
                data_articles(
                    nzb.current_failed_articles,
                    nzb.current_failed_size,
                    nzb.par_current_failed_size,
                ),
            )
        } else {
            0
        };
        if should_wait_for_borrowing_on_par_damage(
            opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE,
            opts.raw_article,
            nzb.dupe_mode,
            crate::dupefallback::has_par2_host(host, nzb),
            nzb.dupe_par_defer_state,
            crate::queuesubsys_host::DP_DEFERRED,
            attempted,
            DUPE_FAILOVER_SAMPLE,
            nzb.dupe_recovered_articles,
            untried,
        ) {
            return false;
        }

        let qlen = host.queue_len(dq_ptr);
        for i in 0..qlen {
            let qp = host.queue_at(dq_ptr, i);
            if qp.is_none() || qp == nzb.ptr {
                continue;
            }
            let queued = host.nzb_snap(qp, false);
            if queued.paused_size < queued.remaining_size
                && crate::dupecoord::same_name_or_key(
                    &queued.name,
                    &queued.dupe_key,
                    &nzb.name,
                    &nzb.dupe_key,
                )
            {
                return false;
            }
        }

        let backup = crate::dupecoord::find_dupe_backup_host(
            host,
            opts,
            dq_ptr,
            nzb.ptr,
            &nzb.name,
            &nzb.dupe_key,
        )
        .filter(|(_, b)| !crate::dupecoord::locked_host(host, b.ptr));
        let damage = format_par_damage_summary(
            certain,
            verdict.damaged_blocks,
            verdict.projected_blocks,
            verdict.block_size as i64,
            verdict.recovery_blocks,
        );
        let Some((_, backup_nzb)) = backup else {
            if crate::pardamage::first_report(nzb.id) {
                let mut msg = nzb.name.clone();
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&damage);
                msg.extend_from_slice(b"; no duplicate in history to fail over to");
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
            }
            return false;
        };

        // A backup the fleet intake is still reading may carry another video, or lend: wait
        // for its verdict (the next round looks again).
        if crate::dupesearch_coord::intake_pending(backup_nzb.id, &nzb.dupe_key) {
            return true;
        }
        // A running extraction is kept: the backup lends what par2 cannot repair.
        if backup_continues_extraction(host, opts, nzb, &backup_nzb) {
            return true;
        }
        let backup_alive = backup_alive_permille(host, backup_nzb.ptr);
        if (!certain && backup_alive < 999)
            || !crate::dupecoord::dupe_failover_warranted_inline(
                nzb.dupe_score,
                0,
                backup_nzb.dupe_score,
            )
        {
            return false;
        }

        note_cannot_continue(host, &opts, &nzb, &backup_nzb);
        let mut msg = b"Failing over ".to_vec();
        msg.extend_from_slice(&nzb.name);
        msg.extend_from_slice(b" to duplicate ");
        msg.extend_from_slice(&backup_nzb.name);
        msg.extend_from_slice(b": ");
        msg.extend_from_slice(&damage);
        msg.extend_from_slice(b", par-repair can't fix it");
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &crate::releasename::log_name(&msg));
        crate::dupefallback::par_damage_forget_all(nzb.id);
        crate::fleethold::park_on_health_host(host, dq_ptr, &nzb);
        true
    }

    /// Articles of the data files `nzb` still downloads (not paused) that were not tried yet.
    /// par2 files are left out: borrowing never fills their articles.
    fn untried_articles(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> i32 {
        (0..host.nzb_file_len(nzb.ptr))
            .map(|i| host.file_snap(host.nzb_file_at(nzb.ptr, i)))
            .filter(|f| !f.paused && !f.deleted && !crate::dupefallback::file_is_par_snap(f))
            .map(|f| {
                (f.total_articles - f.success_articles - f.failed_articles - f.missed_articles)
                    .max(0)
            })
            .fold(0i32, i32::saturating_add)
    }

    /// Whether a doomed download may be failed over by the par2 check (see
    /// `check_dead_download`): not while the weak-primary switch would move it to its fleet's
    /// alt, nor while it is kept to borrow its lost parts, or would be kept
    /// this round (the same [`alt_covers_lost_articles`] the doomed path asks, with its
    /// `dead_rate` and `projected` health), a dead-volume borrow of it runs, or its backup is
    /// still within the classify grace ([`backup_still_unclassified`]).
    fn doomed_par_check_allowed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
        dead_rate: bool,
        tried: i32,
        projected: i32,
    ) -> bool {
        if was_kept_for_borrowing(nzb.id) || crate::postproc::dead_volume_borrow_underway(nzb.id) {
            return false;
        }
        let Some((backup_hi, backup_nzb)) = crate::dupecoord::find_dupe_backup_host(
            host,
            opts,
            dq_ptr,
            nzb.ptr,
            &nzb.name,
            &nzb.dupe_key,
        ) else {
            return true;
        };
        // a weak fleet primary the doomed path switches to its alt (#724): that switch stops
        // its extraction and live writes first, which the par2 failover would skip
        // (as does its continuing at the frontier, which the dead-download check makes; a
        // primary already cut there fails its own articles by design while its alt lends)
        let facts = weak_switch_facts(host, opts, nzb, &backup_nzb, tried, projected);
        if (facts.cut && facts.continuable)
            || matches!(
                weak_primary_switch(&facts),
                WeakSwitch::FailOver | WeakSwitch::ContinueAtFrontier
            )
        {
            return false;
        }
        let backup_kind = crate::donorkind::DonorKind::from_param(
            host.nzb_get_param(backup_nzb.ptr, crate::donorkind::KIND_PARAM)
                .as_deref(),
        );
        if alt_covers_lost_articles(
            dead_rate,
            live_borrowing(opts),
            crate::donorkind::DonorKind::from_param(
                host.nzb_get_param(nzb.ptr, crate::donorkind::KIND_PARAM)
                    .as_deref(),
            ),
            backup_kind,
            projected,
            crate::postproc::dead_volume_borrow_underway(nzb.id),
            nzb.direct_unpack_status == DirectUnpackStatus::Running,
            backup_may_lend_while_extracting(host, backup_nzb.ptr),
            backup_cannot_lend(host, nzb, &backup_nzb),
        ) {
            return false;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        !backup_still_unclassified(
            backup_kind,
            backup_twincheck_labelled(host, backup_nzb.ptr),
            backup_hi.time,
            now,
            crate::dupesearch_coord::intake_pending(backup_nzb.id, &nzb.dupe_key),
            extraction_fed_by_duplicates(opts),
        )
    }

    fn files_tried(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> i32 {
        let mut files = host.nzb_completed_files(nzb.ptr).len() as i32;
        let flen = host.nzb_file_len(nzb.ptr);
        for i in 0..flen {
            let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
            if f.success_articles + f.failed_articles > 0 {
                files += 1;
            }
        }
        files
    }

    fn start_dead_pick_probe(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) {
        if opts.health_check != crate::queuesubsys_host::HC_DUPE
            || !opts.dupe_check
            || nzb.dead_pick_probed
            || !crate::dupecoord::fails_over_snap(opts, nzb)
            || nzb.kind != crate::queuesubsys_host::NK_NZB
            || nzb.deleting
            || nzb.parking
            || nzb.delete_status != DeleteStatus::None
        {
            return;
        }
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DEAD_PICK_PROBED,
            1,
        );
        if nzb.current_success_articles > 0 {
            return;
        }
        if crate::dupecoord::find_dupe_backup_host(
            host,
            opts,
            dq_ptr,
            nzb.ptr,
            &nzb.name,
            &nzb.dupe_key,
        )
        .is_none()
        {
            return;
        }

        let flen = host.nzb_file_len(nzb.ptr);
        let mut files: Vec<crate::queuesubsys_host::FileSnap> = Vec::new();
        for pass in 0..2 {
            if !files.is_empty() {
                break;
            }
            for i in 0..flen {
                let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
                if !f.deleted && !f.paused && f.total_articles > 0 && (pass == 1 || !f.par_file) {
                    files.push(f);
                }
            }
        }

        let total: usize = files.iter().map(|f| f.total_articles.max(0) as usize).sum();
        if total < DUPE_PROBE_MIN_ARTICLES {
            return;
        }

        let indexes = crate::dupeprobe::sample_indexes(total, DUPE_PROBE_SAMPLE_COUNT);
        let mut samples: Vec<(Vec<u8>, Vec<Vec<u8>>)> = Vec::with_capacity(indexes.len());
        for pos in indexes {
            let mut offset = 0usize;
            for f in &mut files {
                let count = f.total_articles.max(0) as usize;
                if pos >= offset + count {
                    offset += count;
                    continue;
                }
                if f.articles_len == 0 {
                    *f = self.ensure_articles_loaded(host, opts, nzb, f.ptr);
                }
                let idx = pos - offset;
                if idx < f.articles_len {
                    let art = host.article_at(f.ptr, idx);
                    let groups = host.file_groups(f.ptr);
                    samples.push((art.message_id, groups));
                }
                break;
            }
        }
        if samples.len() < DUPE_PROBE_MIN_ARTICLES {
            return;
        }

        let mut probe = Box::new(crate::dupeprobe::NzbgetRsDupeProbe::new(
            nzb.id,
            false,
            nzb.dupe_recovered_articles,
        ));
        for (msg_id, groups) in samples {
            probe.add_sample(msg_id, groups);
        }
        #[cfg(unix)]
        if host.use_active {
            crate::dupeprobe::launch_probe_rs(probe);
        }
        #[cfg(not(unix))]
        let _ = probe;
    }

    fn reset_hanging_downloads(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        let opts = host.opts();
        if opts.article_timeout == 0 {
            return;
        }
        host.with_locked_queue(|_| {
            let tm = host.opts().cur_time;
            let n = host.active_downloads_len();
            for i in 0..n {
                let ad = host.active_download_at(i);
                if is_hanging_download(
                    tm,
                    ad.last_update_time,
                    opts.article_timeout,
                    ad.status == crate::queuesubsys_host::AD_RUNNING,
                ) {
                    host.ad_action(ad.ad_ptr, crate::queuesubsys_host::AD_ACT_CANCEL_HANGING, 0);
                }
            }
        });
    }

    fn discard_direct_rename(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb_ptr: NzbRef,
    ) {
        discard_direct_rename_host(host, opts, nzb_ptr);
    }

    pub(crate) fn wakeup(&self) {
        crate::log::log_active_bytes(
            crate::queuesubsys_host::MK_DEBUG,
            b"Waking up QueueCoordinator",
        );
        let mut g = self.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
        self.has_more_jobs
            .store(true, std::sync::atomic::Ordering::SeqCst);
        *g = true;
        self.wait_lock.1.notify_all();
    }

    pub(crate) fn standby_wait(&self, wait_ms: i32) {
        let mut g = self.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
        if !*g && !self.stopped.load(std::sync::atomic::Ordering::SeqCst) && wait_ms > 0 {
            g = self
                .wait_lock
                .1
                .wait_timeout(g, std::time::Duration::from_millis(wait_ms as u64))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *g = false;
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn set_has_more_jobs(&self, more: bool) {
        self.has_more_jobs
            .store(more, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn has_more_jobs(&self) -> bool {
        self.has_more_jobs.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn dq_state_changed(&self) -> bool {
        self.state_changed.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn dq_history_changed(&self) {
        self.history_changed
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn dq_save(&self, host: &crate::queuesubsys_host::QueueSubsysHost, dq_ptr: usize) {
        if self.mass_edit.load(std::sync::atomic::Ordering::SeqCst) {
            self.want_save
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        if host.opts().server_mode {
            let hc = self
                .history_changed
                .load(std::sync::atomic::Ordering::SeqCst);
            let _ = crate::diskstate_format::active_disk_save_queue(host, dq_ptr, hc);
            self.state_changed
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let qlen = host.queue_len(dq_ptr);
        for i in 0..qlen {
            let nzb_ptr = host.queue_at(dq_ptr, i);
            host.nzb_set_int(nzb_ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 0);
        }
        self.want_save
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.history_changed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.wakeup();
    }

    pub(crate) fn dq_save_changed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
    ) {
        if host.opts().server_mode {
            let _ = crate::diskstate_format::active_disk_save_progress(host, dq_ptr);
            self.state_changed
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub(crate) fn dq_edit_entry(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        id: i32,
        action: i32,
        args: Option<&[u8]>,
    ) -> bool {
        crate::queueeditor::edit_entry_host(self, host, dq_ptr, id, action, args)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dq_edit_list(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        ids: Option<&[i32]>,
        names: Option<&[&[u8]]>,
        match_mode: i32,
        action: i32,
        args: Option<&[u8]>,
    ) -> bool {
        // (Nested in an outer edit list, as when a post job deleted from one completes and
        // stops another copy: the outer one's mass edit stays on.)
        let outer = self
            .mass_edit
            .swap(true, std::sync::atomic::Ordering::SeqCst);
        let ret = crate::queueeditor::edit_list_host(
            self, host, dq_ptr, ids, names, match_mode, action, args,
        );
        self.mass_edit
            .store(outer, std::sync::atomic::Ordering::SeqCst);
        if !outer && self.want_save.load(std::sync::atomic::Ordering::SeqCst) {
            self.dq_save(host, dq_ptr);
        }
        ret
    }

    fn find_ad(
        &self,
        ad_ptr: usize,
    ) -> Option<std::sync::Arc<crate::articledl::NzbgetRsArticleDownloader>> {
        let g = self
            .active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        g.iter()
            .find(|a| (std::sync::Arc::as_ptr(a) as usize) == ad_ptr)
            .cloned()
    }

    pub(crate) fn active_downloads_len(&self) -> usize {
        self.active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub(crate) fn active_download_at(
        &self,
        idx: usize,
    ) -> crate::queuesubsys_host::RunningDownloadSnap {
        let ad_opt = self
            .active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(idx)
            .cloned();
        match ad_opt {
            Some(ad) => crate::queuesubsys_host::RunningDownloadSnap {
                ad_ptr: std::sync::Arc::as_ptr(&ad) as usize,
                file_ptr: ad.file_ptr,
                status: ad.status(),
                last_update_time: ad.last_update_time(),
            },
            None => crate::queuesubsys_host::RunningDownloadSnap {
                ad_ptr: 0,
                file_ptr: FileRef::NONE,
                status: 0,
                last_update_time: 0,
            },
        }
    }

    pub(crate) fn ad_snap(&self, ad_ptr: usize) -> crate::queuesubsys_host::ActiveDownloadSnap {
        match self.find_ad(ad_ptr) {
            Some(ad) => ad.snap_for_coord(),
            None => crate::queuesubsys_host::ActiveDownloadSnap::default(),
        }
    }

    pub(crate) fn ad_finish_analyzer(&self, ad_ptr: usize) -> Option<(bool, Vec<u8>, Vec<u8>)> {
        self.find_ad(ad_ptr).and_then(|ad| ad.finish_analyzer())
    }

    pub(crate) fn ad_action(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        ad_ptr: usize,
        action: i32,
    ) -> bool {
        let Some(ad) = self.find_ad(ad_ptr) else {
            return false;
        };
        match action {
            crate::queuesubsys_host::AD_ACT_STOP => {
                ad.stop();
                false
            }
            crate::queuesubsys_host::AD_ACT_LOG_DEBUG => {
                ad.log_debug();
                false
            }
            crate::queuesubsys_host::AD_ACT_ADD_SERVER_STATS => {
                ad.merge_server_stats(host);
                false
            }
            crate::queuesubsys_host::AD_ACT_COMPLETE_FILE_PARTS => ad.complete_file_parts(host),
            crate::queuesubsys_host::AD_ACT_CANCEL_HANGING => {
                ad.cancel_hanging();
                false
            }
            _ => false,
        }
    }

    pub(crate) fn remove_active_download(&self, ad_ptr: usize) {
        let mut g = self
            .active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = g
            .iter()
            .position(|a| (std::sync::Arc::as_ptr(a) as usize) == ad_ptr)
        {
            g.remove(pos);
        }
    }

    pub(crate) fn delete_all_active_downloads(&self) {
        let mut cnt = self
            .active_dl_threads
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        while *cnt > 0 {
            cnt = self
                .active_dl_threads
                .1
                .wait(cnt)
                .unwrap_or_else(|e| e.into_inner());
        }
        drop(cnt);
        let drained: Vec<_> = self
            .active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        drop(drained);
    }

    pub(crate) fn spawn_article_download(
        self: &std::sync::Arc<Self>,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        file_ptr: FileRef,
        article_ptr: usize,
        conn_ptr: usize,
        with_analyzer: bool,
        info_name: &[u8],
    ) {
        let ad = std::sync::Arc::new(crate::articledl::NzbgetRsArticleDownloader::new_for_queue(
            file_ptr,
            article_ptr,
            with_analyzer,
            crate::ffi::build_parcheck_enabled(),
            conn_ptr,
            info_name.to_vec(),
        ));
        let ad_ptr = std::sync::Arc::as_ptr(&ad) as usize;
        host.article_edit(
            article_ptr,
            crate::qarticle::ArticleEdit::Status(ARTICLE_RUNNING),
        );
        let f = host.file_snap(file_ptr);
        host.file_set_int(
            file_ptr,
            crate::queuesubsys_host::FILE_INT_ACTIVE_DOWNLOADS,
            f.active_downloads + 1,
        );
        let n = host.nzb_snap(f.nzb_ptr, false);
        host.nzb_set_int(
            f.nzb_ptr,
            crate::queuesubsys_host::NZB_INT_ACTIVE_DOWNLOADS,
            n.active_downloads + 1,
        );
        self.active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(std::sync::Arc::clone(&ad));
        {
            let mut cnt = self
                .active_dl_threads
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *cnt += 1;
        }
        let coord_cl = std::sync::Arc::clone(self);
        if std::thread::Builder::new()
            .name("article-dl".into())
            .spawn(move || {
                struct ActiveDlGuard(std::sync::Arc<NzbgetRsQueueCoordinator>);
                impl Drop for ActiveDlGuard {
                    fn drop(&mut self) {
                        let mut cnt = self
                            .0
                            .active_dl_threads
                            .0
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        *cnt = cnt.saturating_sub(1);
                        self.0.active_dl_threads.1.notify_all();
                    }
                }
                let _cnt_guard = ActiveDlGuard(std::sync::Arc::clone(&coord_cl));
                {
                    let _tg = crate::ffi::RustThreadGuard::enter(&ad.stopped);
                    if let Some(qh) = crate::ffi::queue_host() {
                        ad.run_with_coord(&qh, &coord_cl);
                    }
                }
                drop(ad);
                crate::ffi::stop_ssl_thread();
            })
            .is_err()
        {
            self.remove_active_download(ad_ptr);
            // The worker never ran, so hand back the connection run() reserved for it.
            if conn_ptr != 0 {
                crate::net::return_connection(conn_ptr as *mut crate::net::PooledNntpConn, false);
            }
            host.article_edit(
                article_ptr,
                crate::qarticle::ArticleEdit::Status(ARTICLE_UNDEFINED),
            );
            host.file_set_int(
                file_ptr,
                crate::queuesubsys_host::FILE_INT_ACTIVE_DOWNLOADS,
                f.active_downloads,
            );
            host.nzb_set_int(
                f.nzb_ptr,
                crate::queuesubsys_host::NZB_INT_ACTIVE_DOWNLOADS,
                n.active_downloads,
            );
            let mut cnt = self
                .active_dl_threads
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *cnt = cnt.saturating_sub(1);
            self.active_dl_threads.1.notify_all();
        }
    }

    pub(crate) fn on_article_finished(
        &self,
        ad: &std::sync::Arc<crate::articledl::NzbgetRsArticleDownloader>,
        host: &crate::queuesubsys_host::QueueSubsysHost,
    ) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Notification from ArticleDownloader received",
        );
        let st = ad.status();
        if st == crate::queuesubsys_host::AD_FINISHED
            || st == crate::queuesubsys_host::AD_FAILED
            || st == crate::queuesubsys_host::AD_RETRY
            || st == crate::queuesubsys_host::AD_FATAL_ERROR
        {
            self.article_completed(host, std::sync::Arc::as_ptr(ad) as usize);
        }
    }
}

#[cfg(unix)]
static DAEMON_QC_INST: std::sync::Mutex<Option<std::sync::Arc<NzbgetRsQueueCoordinator>>> =
    std::sync::Mutex::new(None);
#[cfg(unix)]
static DAEMON_QC_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(unix)]
static DAEMON_QC_HANDLE: std::sync::Mutex<Option<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(None);

#[cfg(unix)]
pub(crate) fn create_daemon_queue_coord() {
    let qc = std::sync::Arc::new(NzbgetRsQueueCoordinator::new());
    crate::ffi::set_active_queue_coord(Some(std::sync::Arc::downgrade(&qc)));
    if let Some(lg) = crate::log::active_log() {
        lg.register_debuggable(5);
    }
    if let Some(qh) = crate::ffi::queue_host() {
        qc.init(&qh);
    }
    if let Ok(mut g) = DAEMON_QC_INST.lock() {
        *g = Some(qc);
    }
}

#[cfg(unix)]
pub(crate) fn start_daemon_queue_coord() {
    let mut hg = DAEMON_QC_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
    if hg.is_some() {
        return;
    }
    let Some(qc) = DAEMON_QC_INST.lock().ok().and_then(|g| g.clone()) else {
        return;
    };
    if !qc.live_startups.reset() {
        return;
    }
    qc.stopped.store(false, std::sync::atomic::Ordering::SeqCst);
    DAEMON_QC_RUNNING.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Ok(h) = std::thread::Builder::new()
        .name("queue-coord".into())
        .spawn(move || {
            struct RunningReset;
            impl Drop for RunningReset {
                fn drop(&mut self) {
                    DAEMON_QC_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
                }
            }
            let _reset = RunningReset;
            let _tg = crate::ffi::RustThreadGuard::enter(&qc.stopped);
            if let Some(qh) = crate::ffi::queue_host() {
                qc.run(&qh);
            }
        })
    {
        *hg = Some(h);
    } else {
        DAEMON_QC_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(unix)]
pub(crate) fn stop_daemon_queue_coord(log_stopping: bool) {
    let Some(qc) = DAEMON_QC_INST.lock().ok().and_then(|g| g.clone()) else {
        return;
    };
    if log_stopping {
        crate::log::log_active_bytes(
            crate::queuesubsys_host::MK_DEBUG,
            b"Stopping QueueCoordinator",
        );
    } else if qc.is_stopped() {
        return;
    }
    if let Some(qh) = crate::ffi::queue_host() {
        qc.stop(&qh);
    } else {
        qc.stop_live_startups();
    }
    let mut g = qc.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
    *g = true;
    qc.wait_lock.1.notify_all();
}

#[cfg(unix)]
pub(crate) fn is_daemon_queue_coord_running() -> bool {
    DAEMON_QC_RUNNING.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(unix)]
pub(crate) fn daemon_queue_coord_has_more_jobs() -> bool {
    DAEMON_QC_INST
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .is_some_and(|qc| qc.has_more_jobs())
}

#[cfg(unix)]
pub(crate) fn shutdown_daemon_queue_coord() {
    stop_daemon_queue_coord(false);
    if let Some(h) = DAEMON_QC_HANDLE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        let _ = h.join();
    }
    DAEMON_QC_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    let qc_opt = DAEMON_QC_INST
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(qc) = qc_opt {
        if let Some(qh) = crate::ffi::queue_host() {
            qc.destroy(&qh);
        }
        if let Some(lg) = crate::log::active_log() {
            lg.unregister_debuggable(5);
        }
        crate::ffi::set_active_queue_coord(None);
    }
}

#[cfg(unix)]
pub(crate) fn notify_from_workstate() {
    if let Some(qc) = crate::ffi::active_queue_coord() {
        crate::log::log_active_bytes(
            crate::queuesubsys_host::MK_DEBUG,
            b"Notification from WorkState received",
        );
        qc.wakeup();
    }
}

#[cfg(unix)]
pub(crate) fn direct_rename_completed_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb_ptr: NzbRef,
) {
    let flen = host.nzb_file_len(nzb_ptr);
    for i in 0..flen {
        let fp = host.nzb_file_at(nzb_ptr, i);
        let f = host.file_snap(fp);
        if opts.server_mode && f.articles_len > 0 {
            host.disk_save_file(fp);
        }
    }

    discard_direct_rename_host(host, opts, nzb_ptr);

    let nzb = host.nzb_snap(nzb_ptr, false);
    host.nzb_set_direct_rename_status(nzb_ptr, DirectRenameStatus::Success);

    if opts.par_check != crate::queuesubsys_host::PC_FORCE {
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_RESUME,
            None,
        );
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_PAUSE_ALL_PARS,
            None,
        );
    }

    if opts.reorder_files {
        let mut msg = b"Reordering files for ".to_vec();
        msg.extend_from_slice(&nzb.name);
        host.nzb_print_message(nzb_ptr, crate::queuesubsys_host::MK_INFO, &msg);
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_SORT_FILES,
            None,
        );
    }
    crate::dupefallback::prioritize_par_index_host(host, nzb_ptr);

    host.nzb_set_int(nzb_ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
    host.dq_save_changed(dq_ptr);

    host.dq_notify(
        dq_ptr,
        crate::queuesubsys_host::ASPECT_NZB_NAMED,
        nzb_ptr,
        FileRef::NONE,
    );
}

#[cfg(unix)]
fn discard_direct_rename_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb_ptr: NzbRef,
) {
    let mut discarded_size = 0i64;
    let mut discarded_count = 0i32;
    let flen = host.nzb_file_len(nzb_ptr);

    for i in 0..flen {
        let fp = host.nzb_file_at(nzb_ptr, i);
        let f = host.file_snap(fp);
        let (discard_par, discard_infos) = eval_discard_direct_rename_file(
            f.par_file,
            f.completed_articles,
            f.active_downloads,
            opts.server_mode,
            f.articles_len > 0,
            opts.continue_partial,
            f.cached_articles,
        );

        if discard_par && host.file_try_acquire_flush_lock(fp) {
            discarded_size += f.success_size;
            discarded_count += 1;
            discard_downloaded_articles_host(host, opts, nzb_ptr, fp);
            host.file_release_flush_lock(fp);
        }

        let f_after = host.file_snap(fp);
        if discard_infos && f_after.articles_len > 0 {
            let nzb = host.nzb_snap(nzb_ptr, false);
            let mut dbg = b"Discarding article infos for ".to_vec();
            dbg.extend_from_slice(&nzb.name);
            dbg.push(b'/');
            dbg.extend_from_slice(&f_after.filename);
            host.log(crate::queuesubsys_host::MK_DEBUG, &dbg);
            host.file_set_int(fp, crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED, 1);
            save_partial_state_host(host, fp);
            host.file_action(fp, crate::queuesubsys_host::FILE_ACT_CLEAR_ARTICLES);
        }
    }

    if discarded_size > 0 {
        let fmt_sz = crate::util::format_size(discarded_size);
        let mut msg = b"Discarded ".to_vec();
        msg.extend_from_slice(&fmt_sz);
        msg.extend_from_slice(
            format!(" from {} files used for direct renaming", discarded_count).as_bytes(),
        );
        host.nzb_print_message(nzb_ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
    }
}

#[cfg(unix)]
fn discard_downloaded_articles_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb_ptr: NzbRef,
    file_ptr: FileRef,
) {
    host.file_subtract_and_clear_server_stats(file_ptr, nzb_ptr);
    let nzb = host.nzb_snap(nzb_ptr, false);
    let file = host.file_snap(file_ptr);
    let (
        next_nzb_rem_size,
        next_nzb_paused_size,
        next_nzb_cur_succ_art,
        next_nzb_cur_succ_size,
        next_nzb_par_cur_succ_size,
        next_nzb_cur_fail_art,
        next_nzb_cur_fail_size,
        next_nzb_par_cur_fail_size,
        next_file_rem_size,
    ) = discard_downloaded_articles_counters(
        nzb.remaining_size,
        nzb.paused_size,
        nzb.current_success_articles,
        nzb.current_success_size,
        nzb.par_current_success_size,
        nzb.current_failed_articles,
        nzb.current_failed_size,
        nzb.par_current_failed_size,
        i32::from(file.paused),
        file.success_articles,
        file.success_size,
        file.failed_articles,
        file.failed_size,
        file.size,
        file.missed_size,
    );

    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_REMAINING_SIZE,
        next_nzb_rem_size,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_PAUSED_SIZE,
        next_nzb_paused_size,
    );
    host.nzb_set_int(
        nzb_ptr,
        crate::queuesubsys_host::NZB_INT_CUR_SUCCESS_ARTICLES,
        next_nzb_cur_succ_art,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_CUR_SUCCESS_SIZE,
        next_nzb_cur_succ_size,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_PAR_CUR_SUCCESS_SIZE,
        next_nzb_par_cur_succ_size,
    );
    host.file_set_i64(file_ptr, crate::queuesubsys_host::FILE_I64_SUCCESS_SIZE, 0);
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_SUCCESS_ARTICLES,
        0,
    );
    host.nzb_set_int(
        nzb_ptr,
        crate::queuesubsys_host::NZB_INT_CUR_FAILED_ARTICLES,
        next_nzb_cur_fail_art,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_CUR_FAILED_SIZE,
        next_nzb_cur_fail_size,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_PAR_CUR_FAILED_SIZE,
        next_nzb_par_cur_fail_size,
    );
    host.file_set_i64(file_ptr, crate::queuesubsys_host::FILE_I64_FAILED_SIZE, 0);
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_FAILED_ARTICLES,
        0,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_COMPLETED_ARTICLES,
        0,
    );
    host.file_set_i64(
        file_ptr,
        crate::queuesubsys_host::FILE_I64_REMAINING_SIZE,
        next_file_rem_size,
    );

    discard_temp_files_host(host, opts, &file);
    host.disk_discard_file(file.id, false, true, false);

    host.file_set_str(
        file_ptr,
        crate::queuesubsys_host::FILE_STR_OUTPUT_FILENAME,
        None,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_OUTPUT_INITIALIZED,
        0,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_CACHED_ARTICLES,
        0,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED,
        0,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_PARTIAL_STATE,
        crate::queuesubsys_host::PARTIAL_NONE,
    );

    if opts.server_mode {
        host.file_action(file_ptr, crate::queuesubsys_host::FILE_ACT_CLEAR_ARTICLES);
    } else {
        host.file_action(
            file_ptr,
            crate::queuesubsys_host::FILE_ACT_RESET_ARTICLES_NONSERV,
        );
    }
}

#[cfg(unix)]
fn discard_temp_files_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    file: &crate::queuesubsys_host::FileSnap,
) {
    let unlink_bytes = |b: &[u8]| {
        let c = crate::queuesubsys_host::c_bytes(b);
        if let Ok(cs) = CStr::from_bytes_with_nul(&c) {
            let _ = crate::fsops::delete_file(cs);
        }
    };
    host.file_discard_temp_articles(file.ptr, !opts.direct_write && !file.force_direct_write);
    if opts.direct_write && !file.output_filename.is_empty() && !file.force_direct_write {
        unlink_bytes(&file.output_filename);
        if file.is_hard_linked && !file.hard_link_path.is_empty() {
            unlink_bytes(&file.hard_link_path);
        }
    }
}

#[cfg(unix)]
fn save_partial_state_host(host: &crate::queuesubsys_host::QueueSubsysHost, file_ptr: FileRef) {
    let file = host.file_snap(file_ptr);
    if file.partial_changed {
        let mut dbg = b"Saving partial state for ".to_vec();
        dbg.extend_from_slice(&file.filename);
        host.log(crate::queuesubsys_host::MK_DEBUG, &dbg);
        if file.partial_state == crate::queuesubsys_host::PARTIAL_COMPLETED {
            host.disk_discard_file(file.id, false, false, true);
        }
        host.disk_save_file_state(file.ptr, false);
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED,
            0,
        );
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_PARTIAL_STATE,
            crate::queuesubsys_host::PARTIAL_PARTIAL,
        );
    }
}

/// NZB parameters for par2 data borrowed from duplicates: `DupeParArticles` counts
/// the articles, `DupeParFiles` lists each par2 file still holding some as
/// `id:count`. They are saved with the item and stay with it in history, so a
/// restart (which forgets the per-file recovered count) never counts a file
/// twice, and a demotion that empties a file drops it from the list.
pub(crate) const PAR_ARTICLES_PARAM: &[u8] = b"DupeParArticles";

/// The fleet identity a part split off a download does not keep: it is not
/// another post of the fleet, so neither its labels nor its rank or fleet
/// membership (`DupeFleetMember` would make it a managed fleet copy forever).
pub(crate) const SPLIT_DROPPED_PARAMS: [&[u8]; 9] = [
    b"DupeFiles",
    b"DupeKind",
    crate::donorkind::KIND_PARAM,
    b"DupeSampled",
    b"DupeSampledOf",
    crate::twinfields::INNER_KEY_PARAM,
    crate::dupesearch_coord::FLEET_PARAM,
    crate::fleethold::MEMBER_PARAM,
    crate::dupesearch_coord::VIDEO_PARAM,
];
pub(crate) const PAR_FILES_PARAM: &[u8] = b"DupeParFiles";

/// Records one par2 article of file `file_id` borrowed from a duplicate
/// (`borrowed`), or taken back after it failed its check (`!borrowed`).
pub(crate) fn note_dupe_par_article(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: NzbRef,
    file_id: i32,
    borrowed: bool,
) {
    let delta = if borrowed { 1 } else { -1 };
    let cur = host.nzb_get_param(nzb_ptr, PAR_ARTICLES_PARAM);
    let next = bumped_count(cur.as_deref(), delta);
    host.nzb_set_param(
        nzb_ptr,
        PAR_ARTICLES_PARAM,
        Some(next.to_string().as_bytes()),
    );
    let mut files = par_files(
        host.nzb_get_param(nzb_ptr, PAR_FILES_PARAM)
            .as_deref()
            .unwrap_or(b""),
    );
    bump_par_file(&mut files, file_id, i64::from(delta));
    host.nzb_set_param(
        nzb_ptr,
        PAR_FILES_PARAM,
        format_par_files(&files).as_deref(),
    );
}

/// Merge: the source's borrowed par2 articles and files move to the destination
/// along with its files.
fn merge_dupe_par_params(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dest_ptr: NzbRef,
    src_ptr: NzbRef,
) {
    let src_articles = bumped_count(
        host.nzb_get_param(src_ptr, PAR_ARTICLES_PARAM).as_deref(),
        0,
    );
    if src_articles > 0 {
        let dest = host.nzb_get_param(dest_ptr, PAR_ARTICLES_PARAM);
        let total = bumped_count(dest.as_deref(), 0).saturating_add(src_articles);
        host.nzb_set_param(
            dest_ptr,
            PAR_ARTICLES_PARAM,
            Some(total.to_string().as_bytes()),
        );
    }
    if let Some(src_files) = host.nzb_get_param(src_ptr, PAR_FILES_PARAM) {
        let mut files = par_files(
            host.nzb_get_param(dest_ptr, PAR_FILES_PARAM)
                .as_deref()
                .unwrap_or(b""),
        );
        for (id, n) in par_files(&src_files) {
            bump_par_file(&mut files, id, n);
        }
        host.nzb_set_param(
            dest_ptr,
            PAR_FILES_PARAM,
            format_par_files(&files).as_deref(),
        );
    }
}

/// The par2 files borrowed from, as `id:count` pairs joined by commas (a bare
/// `id` counts 1). Unreadable entries are skipped and repeated ids add up.
fn par_files(list: &[u8]) -> Vec<(i32, i64)> {
    let mut out: Vec<(i32, i64)> = Vec::new();
    for entry in std::str::from_utf8(list).unwrap_or("").split(',') {
        let mut parts = entry.trim().splitn(2, ':');
        let Some(Ok(id)) = parts.next().map(|t| t.trim().parse::<i32>()) else {
            continue;
        };
        let n = match parts.next() {
            None => 1,
            Some(t) => match t.trim().parse::<i64>() {
                Ok(n) => n,
                Err(_) => continue,
            },
        };
        if n > 0 {
            bump_par_file(&mut out, id, n);
        }
    }
    out
}

/// Adds `delta` articles to file `id`; a file left with none is dropped.
fn bump_par_file(files: &mut Vec<(i32, i64)>, id: i32, delta: i64) {
    match files.iter().position(|(f, _)| *f == id) {
        Some(i) => {
            files[i].1 = files[i].1.saturating_add(delta);
            if files[i].1 <= 0 {
                files.remove(i);
            }
        }
        None if delta > 0 => files.push((id, delta)),
        None => {}
    }
}

/// The param value for `files`, or None (param removed) when there are none.
fn format_par_files(files: &[(i32, i64)]) -> Option<Vec<u8>> {
    if files.is_empty() {
        return None;
    }
    let parts: Vec<String> = files.iter().map(|(id, n)| format!("{id}:{n}")).collect();
    Some(parts.join(",").into_bytes())
}

/// A count parameter's value after adding `delta`, never below 0; a missing or
/// unreadable value counts as 0.
fn bumped_count(cur: Option<&[u8]>, delta: i32) -> i64 {
    cur.and_then(|v| std::str::from_utf8(v).ok())
        .and_then(|t| t.trim().parse::<i64>().ok())
        .unwrap_or(0)
        .max(0)
        .saturating_add(i64::from(delta))
        .max(0)
}

/// The downloaded byte ranges of a file from its articles (finished?,
/// offset, size), adjacent ones merged, capacity trimmed to the length.
fn downloaded_ranges(
    articles: impl Iterator<Item = (bool, i64, i64)>,
) -> Vec<crate::parchecker::ByteRange> {
    let ranges: Vec<crate::parchecker::ByteRange> = articles
        .filter(|&(done, offset, size)| done && offset >= 0 && size > 0)
        .map(|(_, offset, size)| crate::parchecker::ByteRange::new(offset, size))
        .collect();
    let mut merged = par_rs::damage::normalize_ranges(&ranges);
    merged.shrink_to_fit();
    merged
}

#[cfg(test)]
mod tests {

    /// Dune on 6790 (item 2): a 77% primary next to a held 100% alt of the
    /// same video switches while little of it came.
    #[test]
    fn a_weak_primary_switches_to_a_whole_copy_of_the_same_video() {
        use super::{weak_primary_switch, WeakSwitch, WeakSwitchFacts};
        assert_eq!(super::set_stem(b"Show.part07.rar"), super::set_stem(b"show.part12.rar"));
        assert_eq!(super::set_stem(b"Show.r07"), super::set_stem(b"Show.rar"));
        assert_eq!(super::set_stem(b"movie.7z.003"), b"movie.7z".to_vec());
        assert_ne!(super::set_stem(b"Subs.part01.rar"), super::set_stem(b"Show.part01.rar"));
        let dune = WeakSwitchFacts {
            held_in_fleet: true,
            tried: 73477,
            projected: 776,
            backup_alive: 1000,
            same_video: true,
            backup_twin: false,
            primary_held: false,
            backup_locked: false,
            done: 300,
            extraction_verified: false,
            continuable: false,
            cut: false,
        };
        assert_eq!(weak_primary_switch(&dune), WeakSwitch::FailOver);
        assert_eq!(
            weak_primary_switch(&WeakSwitchFacts { continuable: true, ..dune }),
            WeakSwitch::ContinueAtFrontier,
            "the extraction runs and the alt lends live: cut at the frontier, not from zero"
        );
        assert_eq!(
            weak_primary_switch(&WeakSwitchFacts { continuable: true, cut: true, ..dune }),
            WeakSwitch::Stay,
            "cut once"
        );
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { backup_twin: true, ..dune }), WeakSwitch::Stay, "a twin lends one for one");
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { primary_held: true, ..dune }), WeakSwitch::Stay, "failed over already");
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { done: 600, ..dune }), WeakSwitch::Stay, "most of it came: borrow on");
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { same_video: false, ..dune }), WeakSwitch::Stay, "video not proven");
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { backup_alive: 960, ..dune }), WeakSwitch::Stay, "backup not whole");
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { projected: 950, ..dune }), WeakSwitch::Stay, "primary not weak");
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { tried: 50, ..dune }), WeakSwitch::Stay, "too few tried");
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { held_in_fleet: false, ..dune }), WeakSwitch::Stay);
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { backup_locked: true, ..dune }), WeakSwitch::Stay);
        assert_eq!(weak_primary_switch(&WeakSwitchFacts { extraction_verified: true, ..dune }), WeakSwitch::Stay);
    }
    /// Review of b38a5981: a split-off part drops the fleet's rank and its
    /// membership, so it never stays a managed fleet copy.
    #[test]
    fn a_split_part_leaves_its_fleet() {
        assert!(super::SPLIT_DROPPED_PARAMS.contains(&crate::fleethold::MEMBER_PARAM));
        assert!(super::SPLIT_DROPPED_PARAMS.contains(&crate::dupesearch_coord::FLEET_PARAM));
    }

    #[test]
    fn dead_par2_recovery_articles_leave_the_data_projection_alone() {
        // 300 articles failed, 200 of them in par2 files by size: 100 data articles failed.
        assert_eq!(super::data_articles(300, 3_000_000, 2_000_000), 100);
        assert_eq!(super::data_articles(300, 3_000_000, 0), 300);
        assert_eq!(super::data_articles(300, 0, 0), 300);
        assert_eq!(super::data_articles(300, 1_000, 5_000), 0);
        // Terabytes of articles: no overflow on the way.
        assert_eq!(super::data_articles(4_000_000, 3_000_000_000_000, 1_500_000_000_000), 2_000_000);
        // 900 own data articles of 1000 tried: no longer far below a 90% critical health
        // because 400 dead par2 articles were tried first.
        let own = super::data_articles(900, 9_000_000, 0);
        let failed = super::data_articles(500, 5_000_000, 4_000_000);
        let e = super::eval_dead_download(own, 0, failed, 10, 0, false, 900, 0, 5, -1);
        assert_eq!(e.projected, 900);
        assert_eq!(e.far_below, 0);
    }


    #[test]
    fn a_verified_direct_unpack_is_never_failed_over() {
        assert!(direct_unpack_verified(DirectUnpackStatus::Success));
        assert!(!direct_unpack_verified(DirectUnpackStatus::Running));
        assert!(!direct_unpack_verified(DirectUnpackStatus::Failure));
        assert!(!direct_unpack_verified(DirectUnpackStatus::None));
    }

    #[test]
    fn the_dead_check_skips_a_verified_direct_unpack() {
        let mut nzb = crate::queuesubsys_host::NzbSnap {
            kind: crate::queuesubsys_host::NK_NZB,
            direct_unpack_status: DirectUnpackStatus::Running,
            ..Default::default()
        };
        assert!(!dead_check_skipped(&nzb));
        nzb.direct_unpack_status = DirectUnpackStatus::Success;
        assert!(dead_check_skipped(&nzb));
        nzb.direct_unpack_status = DirectUnpackStatus::Failure;
        assert!(!dead_check_skipped(&nzb));
        nzb.parking = true;
        assert!(dead_check_skipped(&nzb));
    }

    #[test]
    fn late_backup_waits_until_classified() {
        use crate::donorkind::DonorKind;
        let parked = 1_000;
        let unclassified = super::backup_still_unclassified;
        assert!(unclassified(DonorKind::Unknown, false, parked, parked + 2, false, true));
        assert!(unclassified(DonorKind::Unknown, false, parked, parked + 119, false, true));
        assert!(!unclassified(DonorKind::Unknown, false, parked, parked + 120, false, true));
        // still being read in the background: held while the check runs, however old
        assert!(unclassified(DonorKind::Unknown, false, parked, parked + 3_600, true, true));
        for kind in [DonorKind::Twin, DonorKind::Borrowable, DonorKind::Unborrowable] {
            assert!(!unclassified(kind, false, parked, parked + 2, false, true));
            assert!(unclassified(kind, false, parked, parked + 3_600, true, true));
        }
        // nothing can lend (RawArticle): an unknown kind is no reason to wait, but the
        // fleet intake still reading the copy's video is (it may drop another video)
        assert!(!unclassified(DonorKind::Unknown, false, parked, parked + 2, false, false));
        assert!(unclassified(DonorKind::Unknown, false, parked, parked + 2, true, false));
        // TwinCheck labelled it (container, alt, ...): its label decides, no grace; the
        // fleet intake still reading the copy's video holds it all the same
        assert!(!unclassified(DonorKind::Unknown, true, parked, parked + 2, false, true));
        assert!(unclassified(DonorKind::Unknown, true, parked, parked + 2, true, true));
    }

    #[test]
    fn lost_articles_expected_at_the_rate_so_far() {
        assert_eq!(super::expected_lost(120, 255, 45), 18);
        assert_eq!(super::expected_lost(10, 85, 15), 1);
        // 31 counted needs one more: expected only once a whole lost article is
        assert_eq!(super::expected_lost(6, 85, 15), 0);
        assert_eq!(super::expected_lost(7, 85, 15), 1);
        assert_eq!(super::expected_lost(0, 85, 15), 0);
        assert_eq!(super::expected_lost(40, 0, 0), 40);
        assert_eq!(super::expected_lost(40, 100, 0), 0);
    }

    #[test]
    fn par_damage_waits_for_the_borrow_sample_only_while_it_can_be_reached() {
        use crate::queuesubsys_host::DP_DEFERRED;
        let wait = |attempted, recovered, untried| {
            super::should_wait_for_borrowing_on_par_damage(
                true, false, DupeMode::Score, true, 0, DP_DEFERRED, attempted, 32, recovered, untried,
            )
        };
        // fewer than 32 tried and enough left to try: borrowing gets its turn
        assert!(wait(10, 0, 100));
        assert!(wait(30, 0, 2));
        // pardamageborrow: 30 counted, too few articles left to reach 32: fail over now
        assert!(!wait(30, 0, 1));
        assert!(!wait(30, 0, 0));
        assert!(!wait(31, 0, 0));
        assert!(wait(31, 0, 1));
        assert!(!wait(16, 0, 0));
        // but never on fewer than half the sample: one failed borrow near the end waits
        assert!(wait(1, 0, 0));
        assert!(wait(15, 7, 3));
        // the sample reached: under half recovered fails over, half or more keeps borrowing
        assert!(!wait(32, 15, 0));
        assert!(wait(32, 16, 0));
        // few tried and nothing left, all recovered: borrowing works, nothing to fail over for
        assert!(wait(4, 2, 0));
        // deferred to par-check: waits whatever the counts
        assert!(super::should_wait_for_borrowing_on_par_damage(
            true, false, DupeMode::Score, true, DP_DEFERRED, DP_DEFERRED, 40, 32, 0, 0,
        ));
    }

    #[test]
    fn classify_grace_waits_only_while_duplicates_feed_the_extraction() {
        use crate::queuesubsys_host::{GlobalOptsSnap, DAF_LIVE, DAF_NONE, DAF_STREAM};
        let opts = |daf, direct_unpack, raw_article| GlobalOptsSnap {
            dupe_article_fallback: daf,
            direct_unpack,
            raw_article,
            ..GlobalOptsSnap::default()
        };
        // live borrowing: DupeArticleFallback=live with DirectUnpack and no RawArticle
        assert!(super::live_borrowing(&opts(DAF_LIVE, true, false)));
        assert!(!super::live_borrowing(&opts(DAF_NONE, true, false)));
        assert!(!super::live_borrowing(&opts(DAF_STREAM, true, false)));
        assert!(!super::live_borrowing(&opts(DAF_LIVE, false, false)));
        assert!(!super::live_borrowing(&opts(DAF_LIVE, true, true)));
        // the grace: live borrowing, or live stream repair patching the extraction; not
        // with DupeArticleFallback=no (B48/B49 projectedswap, parlesscritical), without
        // DirectUnpack, or with RawArticle (dupefailoverraw)
        assert!(super::extraction_fed_by_duplicates(&opts(DAF_LIVE, true, false)));
        assert!(super::extraction_fed_by_duplicates(&opts(DAF_STREAM, true, false)));
        assert!(!super::extraction_fed_by_duplicates(&opts(DAF_NONE, true, false)));
        assert!(!super::extraction_fed_by_duplicates(&opts(DAF_STREAM, false, false)));
        assert!(!super::extraction_fed_by_duplicates(&opts(DAF_LIVE, true, true)));
    }

    #[test]
    fn bumped_count_starts_from_zero_and_never_goes_negative() {
        assert_eq!(bumped_count(None, 1), 1);
        assert_eq!(bumped_count(Some(b"41"), 1), 42);
        assert_eq!(bumped_count(Some(b" 7 "), 3), 10);
        assert_eq!(bumped_count(Some(b"junk"), 2), 2);
        assert_eq!(bumped_count(Some(b"-5"), 1), 1);
        assert_eq!(bumped_count(Some(b"9"), -4), 5);
        assert_eq!(bumped_count(Some(b"1"), -1), 0);
        assert_eq!(bumped_count(None, -1), 0);
    }

    #[test]
    fn par_files_parse_counts_and_skip_junk() {
        assert_eq!(
            par_files(b"12:3, 15,x,,7:0,9:y,12:1"),
            vec![(12, 4), (15, 1)]
        );
        assert_eq!(par_files(b""), Vec::<(i32, i64)>::new());
    }

    #[test]
    fn par_files_count_per_file_and_drop_emptied_ones() {
        let mut f = par_files(b"");
        bump_par_file(&mut f, 12, 1);
        bump_par_file(&mut f, 12, 1);
        bump_par_file(&mut f, 15, 1);
        assert_eq!(format_par_files(&f), Some(b"12:2,15:1".to_vec()));
        bump_par_file(&mut f, 15, -1);
        bump_par_file(&mut f, 99, -1);
        assert_eq!(format_par_files(&f), Some(b"12:2".to_vec()));
        bump_par_file(&mut f, 12, -2);
        assert_eq!(format_par_files(&f), None);
    }

    #[test]
    fn live_par_memory_refusal_is_told_in_mb() {
        let err = crate::parchecker::RepairError::StreamingMemoryLimit {
            context: "bounded live PAR startup",
            required_bytes: 152_502_272,
            limit_bytes: 128 * 1024 * 1024,
        };
        let msg = super::NzbgetRsQueueCoordinator::live_par_deferred_message(&err);
        assert!(msg.contains("need 146 MB of ParBuffer"), "{msg}");
        assert!(msg.contains("128 MB is available"), "{msg}");
        assert!(msg.contains("set ParBuffer to 146 MB or more"), "{msg}");
        assert!(!msg.contains("bytes"), "{msg}");
        // a busy pool or a fixed cap is not a ParBuffer question
        let busy = crate::parchecker::RepairError::StreamingMemoryLimit {
            context: "shared live PAR reservation",
            required_bytes: 128 * 1024 * 1024,
            limit_bytes: 500 * 1024 * 1024,
        };
        let msg = super::NzbgetRsQueueCoordinator::live_par_deferred_message(&busy);
        assert!(!msg.contains("set ParBuffer"), "{msg}");
        assert!(msg.contains("shared live PAR reservation"), "{msg}");
    }

    #[test]
    fn downloaded_ranges_merge_adjacent_articles() {
        // a 7,315-article volume with two missing articles: three ranges
        let art = 739_786i64;
        let rows = (0..7315i64).map(|i| (i != 100 && i != 5000, i * art, art));
        let r = super::downloaded_ranges(rows);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], crate::parchecker::ByteRange::new(0, 100 * art));
        assert_eq!(r[1].offset, 101 * art);
        assert!(r.capacity() == r.len());
    }

    #[test]
    fn keeping_for_borrowing_is_forgotten_when_the_download_leaves_the_queue() {
        let id = -5_220_005;
        assert!(note_kept_for_borrowing(id));
        assert!(!note_kept_for_borrowing(id)); // said once
        TOLD_CANNOT_LEND
            .lock()
            .unwrap()
            .get_or_insert_with(Default::default)
            .insert((id, 7));
        crate::postproc::forget_dead_volume_verdicts(id);
        assert!(!was_kept_for_borrowing(id));
        assert!(!TOLD_CANNOT_LEND.lock().unwrap().as_ref().is_some_and(|g| g.contains(&(id, 7))));
        assert!(note_kept_for_borrowing(id)); // a new stay in the queue says it again
        forget_kept_for_borrowing(id);
    }

    #[test]
    fn lost_articles_an_alt_can_lend_keep_the_download() {
        use crate::donorkind::DonorKind::*;
        assert!(alt_covers_lost_articles(
            false, true, Borrowable, Borrowable, 952, false, false, true, false
        ));
        assert!(alt_covers_lost_articles(
            false, true, Unknown, Twin, 600, false, false, true, false
        ));
        // an encrypted store copy whose nzb password opens it lends like a plain one,
        // even unlabelled and before its borrow starts
        assert!(alt_covers_lost_articles(
            false, true, Borrowable, EncryptedBorrowable, 952, false, false, false, false
        ));
        // Mostly gone, no live borrowing, an unborrowable backup or download: fail over.
        assert!(!alt_covers_lost_articles(
            true, true, Borrowable, Borrowable, 952, false, false, true, false
        ));
        assert!(!alt_covers_lost_articles(
            false, false, Borrowable, Borrowable, 952, false, false, true, false
        ));
        assert!(!alt_covers_lost_articles(
            false,
            true,
            Borrowable,
            Unborrowable,
            952,
            false,
            false,
            true,
            false
        ));
        assert!(!alt_covers_lost_articles(
            false, true, Borrowable, Unknown, 952, false, false, true, false
        ));
        assert!(!alt_covers_lost_articles(
            false,
            true,
            Unborrowable,
            Borrowable,
            952,
            false,
            false,
            true,
            false
        ));
        assert!(!alt_covers_lost_articles(
            false, true, Borrowable, Borrowable, 499, false, false, true, false
        ));
        // A backup of unknown kind lends once a borrow from it is underway ...
        assert!(alt_covers_lost_articles(
            false, true, Unknown, Unknown, 952, true, false, true, false
        ));
        assert!(!alt_covers_lost_articles(
            false,
            true,
            Unknown,
            Unborrowable,
            952,
            true,
            false,
            true,
            false
        ));
        assert!(!alt_covers_lost_articles(
            true, true, Unknown, Unknown, 952, true, false, true, false
        ));
        // ... or while direct unpack runs if labelled an alt (no par2: the first dead volume
        // trips the failed-in-a-row check before it ends), also one a fleet found unborrowable
        // by range (its video compressed: zipfast restarts it near the frontier) until that
        // restart is refused (mismatched), but never when live borrowing is off.
        assert!(alt_covers_lost_articles(
            false, true, Borrowable, Unknown, 952, false, true, true, false
        ));
        assert!(alt_covers_lost_articles(
            false, true, Borrowable, Unborrowable, 952, false, true, true, false
        ));
        assert!(!alt_covers_lost_articles(
            false, true, Borrowable, Unborrowable, 952, false, true, true, true
        ));
        assert!(!alt_covers_lost_articles(
            false, true, Borrowable, Unborrowable, 952, false, true, false, false
        ));
        assert!(!alt_covers_lost_articles(
            false, false, Borrowable, Unknown, 952, false, true, true, false
        ));
        // Mostly gone while direct unpack runs: the backup feeds the rest of the extraction
        // instead of a failover starting it over, a borrowable or labelled backup alike.
        assert!(alt_covers_lost_articles(
            true, true, Borrowable, Unknown, 952, false, true, true, false
        ));
        assert!(alt_covers_lost_articles(
            true, true, Borrowable, Borrowable, 100, false, true, false, false
        ));
        assert!(alt_covers_lost_articles(
            false, true, Unknown, Borrowable, 0, false, true, false, false
        ));
        // ... but never from an unlabelled unborrowable or a mismatched backup (a compressed
        // one zipfast could not restart counts as mismatched), an unborrowable download, or
        // with live borrowing off.
        assert!(!alt_covers_lost_articles(
            true, true, Borrowable, Unborrowable, 100, false, true, false, false
        ));
        assert!(!alt_covers_lost_articles(
            true, true, Borrowable, Unborrowable, 100, false, true, true, true
        ));
        assert!(!alt_covers_lost_articles(
            true, true, Borrowable, Borrowable, 100, false, true, true, true
        ));
        assert!(!alt_covers_lost_articles(
            true, true, Unborrowable, Borrowable, 100, false, true, true, false
        ));
        assert!(!alt_covers_lost_articles(
            true, false, Borrowable, Borrowable, 100, false, true, true, false
        ));
        // An unlabelled backup (not classed the same video in another container) still
        // fails over while extracting.
        assert!(!alt_covers_lost_articles(
            false, true, Borrowable, Unknown, 952, false, true, false, false
        ));
        // A backup whose video the dead-volume borrow found to differ lends nothing, a
        // labelled alt with a borrow underway or a borrowable one included.
        assert!(!alt_covers_lost_articles(
            false, true, Borrowable, Unknown, 952, true, true, true, true
        ));
        assert!(!alt_covers_lost_articles(
            false, true, Borrowable, Borrowable, 952, false, false, true, true
        ));
    }

    #[test]
    fn dupe_mass_failure_thresholds() {
        assert!(!dupe_mass_failure(1000, 3));
        assert!(!dupe_mass_failure(1000, 19));
        assert!(dupe_mass_failure(1000, 20));
        assert!(!dupe_mass_failure(100_000, 500));
        assert!(dupe_mass_failure(100_000, 2000));
        assert!(!dupe_mass_failure(10, 5));
    }

    use super::*;

    #[test]
    fn a_copy_in_stream_repair_lets_other_copies_download_with_dupe_health() {
        use crate::queuesubsys_host::{HC_DUPE, HC_PARK};
        let lets = |post, stage, a: &str, b: &str, inter: &str, hc: i32| {
            repair_lets_other_copies_download(post, stage, a.as_bytes(), b.as_bytes(), || {
                (hc, inter.as_bytes().to_vec())
            })
        };
        let (a, b, inter) = ("/i/Rel.#1", "/i/Rel.#2", "/i");
        assert!(lets(true, PostStage::StreamRepairing, a, b, inter, HC_DUPE));
        assert!(lets(true, PostStage::StreamRepairing, a, b, inter, HC_PARK));
        // any other stage still holds them back, as does a health check without dupes ...
        assert!(!lets(true, PostStage::Unpacking, a, b, inter, HC_DUPE));
        assert!(!lets(false, PostStage::StreamRepairing, a, b, inter, HC_DUPE));
        assert!(!lets(
            true,
            PostStage::StreamRepairing,
            a,
            b,
            inter,
            crate::queuesubsys_host::HC_NONE
        ));
        // ... or folders not both under InterDir: shared (the same name, no InterDir), or
        // final folders that would both land files in the destination
        assert!(!lets(true, PostStage::StreamRepairing, "/d/Rel", "/d/Rel", "", HC_DUPE));
        assert!(!lets(true, PostStage::StreamRepairing, "/d/RelA", "/d/RelB", "", HC_DUPE));
        assert!(!lets(true, PostStage::StreamRepairing, a, "/d/RelB", inter, HC_DUPE));
    }

    #[test]
    fn test_queue_coordinator_helpers() {
        assert!(is_data_filename(b"Movie.2026.mkv"));
        assert!(is_data_filename(b"archive.r00"));
        assert!(is_data_filename(b"split.001"));
        assert!(!is_data_filename(b"checksum.par2"));

        assert!(is_side_file(b"release.NFO", b""));
        assert!(is_side_file(b"sample.srs", b""));
        assert!(is_side_file(b"extra.sfv", b".par2, .nzb"));
        assert!(!is_side_file(b"repair.par2", b".par2, .nfo"));

        let eval = eval_dead_download(0, 0, 250, 10, 45, true, 900, 20, 4, 15);
        assert_eq!(eval.dead_rate, 1);
        assert_eq!(eval.dead_run, 1);
        assert_eq!(eval.dead, 1);
        assert_eq!(eval.hopeless, 1);

        let names: &[&[u8]] = &[b"a.rar", b"b.rar", b"a.rar"];
        let sizes: &[i64] = &[100, 200, 300];
        let mut out = [0usize; 3];
        assert_eq!(find_dupe_file_indices(names, sizes, &mut out), 1);
        assert_eq!(out[0], 0);

        let (uniq, formatted) = format_donor_id_list(&[3, 1, 3, 2]);
        assert_eq!(uniq, vec![1, 2, 3]);
        assert_eq!(formatted, b"1, 2, 3");

        let mut d = NzbgetRsNzbMergeSplitCounters {
            file_count: 2,
            size: 100,
            min_time: 50,
            max_time: 60,
            ..Default::default()
        };
        let s = NzbgetRsNzbMergeSplitCounters {
            file_count: 3,
            size: 200,
            min_time: 40,
            max_time: 80,
            ..Default::default()
        };
        merge_counters(&mut d, &s);
        assert_eq!(
            (d.file_count, d.size, d.min_time, d.max_time),
            (5, 300, 40, 80)
        );
    }

    #[test]
    fn propagation_wait_disabled_test() {
        let now: i64 = 1_760_000_000;
        assert!(!is_propagation_wait(now, 0, now));
        assert!(!is_propagation_wait(now + 60, 0, now));
    }

    #[test]
    fn propagation_wait_test() {
        let now: i64 = 1_760_000_000;
        let delay: i32 = 15 * 60;

        // posted just now or within the delay: held back
        assert!(is_propagation_wait(now, delay, now));
        assert!(is_propagation_wait(now - 14 * 60, delay, now));
        assert!(is_propagation_wait(now - delay as i64, delay, now));

        // posted before the delay: downloadable
        assert!(!is_propagation_wait(now - delay as i64 - 1, delay, now));
        assert!(!is_propagation_wait(now - 3600, delay, now));
    }

    #[test]
    fn propagation_wait_per_file_test() {
        let now: i64 = 1_760_000_000;
        let delay: i32 = 15 * 60;
        let mkv_posted = now - 3600;
        let nfo_posted = now - 60;

        assert!(!is_propagation_wait(mkv_posted, delay, now));
        assert!(is_propagation_wait(nfo_posted, delay, now));
        assert!(!is_propagation_wait(
            nfo_posted,
            delay,
            nfo_posted + delay as i64 + 1
        ));
    }

    #[cfg(unix)]
    #[test]
    fn reset_hanging_downloads_uses_minimal_running_snapshot() {
        use std::ffi::{c_char, c_int, c_void};
        struct State {
            cancelled: Vec<usize>,
            saw_non_null_strs_on_running_snap: bool,
        }
        unsafe extern "C" fn cb(
            ctx: *mut c_void,
            op: c_int,
            a1: c_int,
            _a2: c_int,
            p1: usize,
            p2: usize,
            _s1: *const c_char,
            _s2: *const c_char,
            ints: *mut c_int,
            i64s: *mut i64,
            strs: *mut *const c_char,
        ) -> usize {
            let st = &mut *(ctx as *mut State);
            match op {
                1 => {
                    *ints.add(13) = 30; // article_timeout
                    *i64s = 1_000; // cur_time
                    0
                }
                20 => {
                    let f: unsafe extern "C" fn(*mut c_void, usize) = std::mem::transmute(p1);
                    f(p2 as *mut c_void, 0xDEAD);
                    0
                }
                50 => {
                    if a1 < 0 {
                        return 2;
                    }
                    if !strs.is_null() {
                        st.saw_non_null_strs_on_running_snap = true;
                    }
                    if a1 == 0 {
                        *ints = crate::queuesubsys_host::AD_RUNNING;
                        *i64s = 0x100;
                        *i64s.add(1) = 990; // fresh: not hanging
                        0xAD01
                    } else {
                        *ints = crate::queuesubsys_host::AD_RUNNING;
                        *i64s = 0x200;
                        *i64s.add(1) = 900; // stale > 30s: hanging
                        0xAD02
                    }
                }
                51 => {
                    if a1 == crate::queuesubsys_host::AD_ACT_CANCEL_HANGING {
                        st.cancelled.push(p1);
                    }
                    0
                }
                _ => 0,
            }
        }
        let mut st = State {
            cancelled: Vec::new(),
            saw_non_null_strs_on_running_snap: false,
        };
        let host = crate::queuesubsys_host::QueueSubsysHost::new_mock(
            &mut st as *mut State as *mut c_void,
            cb,
        );
        let coord = NzbgetRsQueueCoordinator::new();
        coord.reset_hanging_downloads(&host);
        assert!(!st.saw_non_null_strs_on_running_snap);
        assert_eq!(st.cancelled, vec![0xAD02]);
    }
}

/// Stores the share of a download's tried articles that arrived as the health it
/// was judged on ([`crate::dupecoord::PROJECTED_PARAM`]): its own Health counts
/// every untried article as good.
#[cfg(unix)]
fn set_projected_from_tried(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) {
    let projected =
        crate::dupecoord::arrived_percent(nzb.current_success_articles, nzb.current_failed_articles);
    host.nzb_set_param(
        nzb.ptr,
        crate::dupecoord::PROJECTED_PARAM,
        Some(format!("{projected:.1}").as_bytes()),
    );
}
