//! End-to-end Rust implementation of `DirectRenamer` (`daemon/queue/DirectRenamer.cpp`):
//! regular and par2 file renaming (`regular_name`, `par_name`, `par_name_formatted`),
//! par2 set rename check (`need_rename_pars`), article hash16k / par-flag delta
//! evaluation (`eval_direct_rename_article`), and full queue/loader orchestration.

use std::ffi::CStr;

/// `BuildNewRegularName`: the par2 file name the first par2 hash equal to
/// `hash16k` names, if that differs from `old_name` in more than case.
/// `hashes` yields `(filename, hash)` in list order; the result is the
/// hash's index.
pub fn regular_name<'a>(
    old_name: &CStr,
    hash16k: &CStr,
    hashes: impl Iterator<Item = (&'a CStr, &'a CStr)>,
) -> Option<usize> {
    if hash16k.is_empty() {
        return None;
    }
    let (index, (filename, _)) = hashes.enumerate().find(|(_, (_, hash))| *hash == hash16k)?;
    (!old_name
        .to_bytes()
        .eq_ignore_ascii_case(filename.to_bytes()))
    .then_some(index)
}

/// `ostream << setfill('0') << setw(3) << vol`: padded on the left, also
/// before a minus sign.
fn push_vol(out: &mut Vec<u8>, vol: i32) {
    let digits = vol.to_string();
    out.extend(std::iter::repeat_n(
        b'0',
        3usize.saturating_sub(digits.len()),
    ));
    out.extend_from_slice(digits.as_bytes());
}

/// `BuildNewParName`: `set_id` plus the old `".volNN+NN.par2"` suffix when that
/// is free in `dest_dir`, else the next free `"<set_id>.volNNN+01.PAR2"`,
/// counting `vol` up for every name tried.
pub fn par_name(
    old_name: &[u8],
    dest_dir: &[u8],
    set_id: &[u8],
    separator: u8,
    vol: &mut i32,
    exists: &mut dyn FnMut(&[u8]) -> bool,
) -> Vec<u8> {
    par_name_formatted(
        old_name,
        dest_dir,
        set_id,
        separator,
        vol,
        exists,
        &mut |vol| {
            let mut name = set_id.to_vec();
            name.extend_from_slice(b".vol");
            push_vol(&mut name, vol);
            name.extend_from_slice(b"+01.PAR2");
            Some(name)
        },
    )
}

/// `BuildNewParName` checking candidate file existence directly on disk.
pub fn par_name_on_disk(
    old_name: &[u8],
    dest_dir: &[u8],
    set_id: &[u8],
    separator: u8,
    vol: &mut i32,
) -> Vec<u8> {
    let mut exists = |path: &[u8]| {
        #[cfg(unix)]
        let p = {
            use std::os::unix::ffi::OsStrExt;
            std::path::Path::new(std::ffi::OsStr::from_bytes(path))
        };
        #[cfg(not(unix))]
        let p = std::path::Path::new(std::str::from_utf8(path).unwrap_or(""));
        p.is_file()
    };
    par_name(old_name, dest_dir, set_id, separator, vol, &mut exists)
}

/// A non-classic C++ locale may group digits or install a custom `num_put`
/// facet. Let that stream format candidates; `None` stops after an exception.
pub fn par_name_formatted(
    old_name: &[u8],
    dest_dir: &[u8],
    set_id: &[u8],
    separator: u8,
    vol: &mut i32,
    exists: &mut dyn FnMut(&[u8]) -> bool,
    format: &mut dyn FnMut(i32) -> Option<Vec<u8>>,
) -> Vec<u8> {
    let full = |name: &[u8]| [dest_dir, &[separator], name].concat();
    let suffix = old_name
        .windows(4)
        .position(|w| w == b".vol")
        .map(|i| &old_name[i..]);
    if let Some(suffix) = suffix {
        let extension = &suffix[suffix.iter().rposition(|&b| b == b'.').unwrap_or(0)..];
        let ext_end = extension
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(extension.len());
        if extension[..ext_end].eq_ignore_ascii_case(b".par2") {
            let name = [set_id, suffix].concat();
            if !exists(&full(&name)) {
                return name;
            }
        }
    }
    loop {
        let Some(name) = format(*vol) else {
            return Vec::new();
        };
        *vol = vol.wrapping_add(1);
        if !exists(&full(&name)) {
            return name;
        }
    }
}

/// `NeedRenamePars`: a par2 file without the `".par2"` extension (any case), or
/// two par2 files of one set that `ParParser::SameParCollection` does not put
/// in the same collection.
pub fn need_rename_pars(pars: &[(Option<&CStr>, &CStr)]) -> bool {
    pars.iter().enumerate().any(|(i, &(name, set_id))| {
        let Some(name) = name else { return true };
        !crate::util::ends_with(name.to_bytes(), b".par2", false)
            || pars[i + 1..].iter().any(|&(name2, set_id2)| {
                set_id == set_id2
                    && name2.is_none_or(|name2| {
                        !crate::parparser::same_par_collection(
                            name.to_bytes(),
                            name2.to_bytes(),
                            false,
                        )
                    })
            })
    })
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsDirectRenameParDelta {
    pub apply_hash16k: i32,
    pub par_flag_changed: i32,
    pub par_size: i64,
    pub par_current_success_size: i64,
    pub par_current_failed_size: i64,
    pub par_failed_size: i64,
    pub remaining_par_count: i32,
    pub unpause_non_par: i32,
}

/// Evaluates `DirectRenamer::ArticleDownloaded` hash16k eligibility and par-flag transition deltas.
#[allow(clippy::too_many_arguments)]
pub fn eval_direct_rename_article(
    article_size: i32,
    article_count: usize,
    old_par_file: bool,
    new_par_file: bool,
    file_paused: bool,
    file_size: i64,
    file_success_size: i64,
    file_failed_size: i64,
    file_missed_size: i64,
    par_size: i64,
    par_current_success_size: i64,
    par_current_failed_size: i64,
    par_failed_size: i64,
    remaining_par_count: i32,
) -> RsDirectRenameParDelta {
    let apply_hash16k = if article_size >= 16 * 1024 || article_count == 1 {
        1
    } else {
        0
    };
    if old_par_file == new_par_file {
        return RsDirectRenameParDelta {
            apply_hash16k,
            par_flag_changed: 0,
            par_size,
            par_current_success_size,
            par_current_failed_size,
            par_failed_size,
            remaining_par_count,
            unpause_non_par: 0,
        };
    }

    let delta: i64 = if new_par_file { 1 } else { -1 };
    let count_delta: i32 = if new_par_file { 1 } else { -1 };
    let unpause_non_par = if !new_par_file && file_paused { 1 } else { 0 };

    RsDirectRenameParDelta {
        apply_hash16k,
        par_flag_changed: 1,
        par_size: par_size + file_size * delta,
        par_current_success_size: par_current_success_size + file_success_size * delta,
        par_current_failed_size: par_current_failed_size
            + file_failed_size * delta
            + file_missed_size * delta,
        par_failed_size: par_failed_size + file_missed_size * delta,
        remaining_par_count: remaining_par_count + count_delta,
        unpause_non_par,
    }
}

/// Resolves the old output path in `DirectRenamer::RenameFilesInProgress`.
pub fn resolve_old_output_path(
    curr_output_filename: &[u8],
    dest_dir: &[u8],
    sep: u8,
    filename: &[u8],
) -> Vec<u8> {
    if !curr_output_filename.is_empty() {
        return curr_output_filename.to_vec();
    }
    let mut out = Vec::with_capacity(dest_dir.len() + 1 + filename.len());
    out.extend_from_slice(dest_dir);
    out.push(sep);
    out.extend_from_slice(filename);
    out
}

#[cfg(unix)]
#[inline]
fn cut_nul(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

#[cfg(unix)]
#[inline]
fn to_cstring(bytes: &[u8]) -> std::ffi::CString {
    std::ffi::CString::new(cut_nul(bytes)).unwrap_or_default()
}

#[cfg(unix)]
pub(crate) fn article_downloaded_host(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    file_ptr: usize,
    article_size: i32,
    ad_ptr: usize,
) {
    use crate::queuesubsys_host::*;

    let file = host.file_snap(file_ptr);
    let mut dbg = b"Applying analyzer data ".to_vec();
    dbg.extend_from_slice(&file.filename);
    dbg.extend_from_slice(b" for ");
    host.log(MK_DEBUG, &dbg);

    let Some((new_par_file, hash16k, par_set_id)) = host.ad_finish_analyzer(ad_ptr) else {
        return;
    };

    let nzb_ptr = file.nzb_ptr;
    let nzb = host.nzb_snap(nzb_ptr, false);

    let delta = eval_direct_rename_article(
        article_size,
        file.articles_len,
        file.par_file,
        new_par_file,
        file.paused,
        file.size,
        file.success_size,
        file.failed_size,
        file.missed_size,
        nzb.par_size,
        nzb.par_current_success_size,
        nzb.par_current_failed_size,
        nzb.par_failed_size,
        nzb.remaining_par_count,
    );

    if delta.apply_hash16k != 0 {
        host.file_set_str(file_ptr, FILE_STR_HASH16K, Some(&hash16k));
        let mut msg = b"file: ".to_vec();
        msg.extend_from_slice(&file.filename);
        msg.extend_from_slice(b"; article-hash16k: ");
        msg.extend_from_slice(&hash16k);
        host.log(MK_DEBUG, &msg);
    }

    let mut det_msg = b"Detected ".to_vec();
    det_msg.extend_from_slice(if new_par_file {
        b"par2-file "
    } else {
        b"non-par2-file "
    });
    det_msg.extend_from_slice(&file.filename);
    host.nzb_print_message(nzb_ptr, MK_DETAIL, &det_msg);

    if delta.par_flag_changed != 0 {
        let mut chg_msg = b"Changing par2-flag for ".to_vec();
        chg_msg.extend_from_slice(&file.filename);
        host.log(MK_DEBUG, &chg_msg);
        host.file_set_int(file_ptr, FILE_INT_PAR_FILE, i32::from(new_par_file));

        host.nzb_set_i64(nzb_ptr, NZB_I64_PAR_SIZE, delta.par_size);
        host.nzb_set_i64(
            nzb_ptr,
            NZB_I64_PAR_CUR_SUCCESS_SIZE,
            delta.par_current_success_size,
        );
        host.nzb_set_i64(
            nzb_ptr,
            NZB_I64_PAR_CUR_FAILED_SIZE,
            delta.par_current_failed_size,
        );
        host.nzb_set_i64(nzb_ptr, NZB_I64_PAR_FAILED_SIZE, delta.par_failed_size);
        host.nzb_set_int(
            nzb_ptr,
            NZB_INT_REMAINING_PAR_COUNT,
            delta.remaining_par_count,
        );

        if delta.unpause_non_par != 0 {
            let mut res_msg = b"Resuming non-par2-file ".to_vec();
            res_msg.extend_from_slice(&file.filename);
            host.nzb_print_message(nzb_ptr, MK_INFO, &res_msg);
            host.file_set_int(file_ptr, FILE_INT_PAUSED, 0);
        }

        host.nzb_set_int(nzb_ptr, NZB_INT_CHANGED, 1);
        host.dq_save_changed(dq_ptr);
    }

    if new_par_file {
        host.file_set_str(file_ptr, FILE_STR_PAR_SET_ID, Some(&par_set_id));
        let mut sid_msg = b"file: ".to_vec();
        sid_msg.extend_from_slice(&file.filename);
        sid_msg.extend_from_slice(b"; setid: ");
        sid_msg.extend_from_slice(&par_set_id);
        host.log(MK_DEBUG, &sid_msg);
    }

    check_state_host(coord, host, opts, dq_ptr, nzb_ptr);
}

#[cfg(unix)]
pub(crate) fn file_downloaded_host(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    file_ptr: usize,
) {
    let file = host.file_snap(file_ptr);
    if file.nzb_ptr != 0 {
        check_state_host(coord, host, opts, dq_ptr, file.nzb_ptr);
    }
}

#[cfg(unix)]
fn check_state_host(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb_ptr: usize,
) {
    use crate::queuesubsys_host::*;

    if !opts.parcheck_enabled || nzb_ptr == 0 {
        return;
    }

    let nzb = host.nzb_snap(nzb_ptr, false);
    if nzb.direct_rename_status > TS_RUNNING {
        return;
    }

    let flen = host.nzb_file_len(nzb_ptr);
    let mut files = Vec::with_capacity(flen);
    for i in 0..flen {
        let f = host.file_snap(host.nzb_file_at(nzb_ptr, i));
        if f.hash16k.is_empty() || (f.par_file && f.par_set_id.is_empty()) {
            return;
        }
        files.push(f);
    }

    let completed = host.nzb_completed_files(nzb_ptr);
    for cf in &completed {
        if cf.hash16k.is_empty() || (cf.par_file && cf.par_set_id.is_empty()) {
            return;
        }
    }

    let mut waiting_par = nzb.waiting_par;
    if !waiting_par {
        unpause_pars_host(host, nzb_ptr, &files, &completed);
        host.nzb_set_int(nzb_ptr, NZB_INT_WAITING_PAR, 1);
        host.nzb_set_int(nzb_ptr, NZB_INT_CHANGED, 1);
        host.dq_save_changed(dq_ptr);
        waiting_par = true;
    }

    if waiting_par && !nzb.loading_par {
        let flen_now = host.nzb_file_len(nzb_ptr);
        let mut any_extra = false;
        for i in 0..flen_now {
            if host.file_snap(host.nzb_file_at(nzb_ptr, i)).extra_priority {
                any_extra = true;
                break;
            }
        }
        if !any_extra {
            host.nzb_set_int(nzb_ptr, NZB_INT_LOADING_PAR, 1);
            start_par_loader_host(coord, host, opts, nzb_ptr);
        }
    }
}

#[cfg(unix)]
fn unpause_pars_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
    files: &[crate::queuesubsys_host::FileSnap],
    completed: &[crate::queuesubsys_host::CompletedFileSnap],
) {
    use crate::queuesubsys_host::*;

    struct ParCand {
        id: i32,
        set_id: Vec<u8>,
        completed: bool,
    }

    let mut par_files: Vec<ParCand> = Vec::new();
    for f in files {
        if f.par_file {
            par_files.push(ParCand {
                id: f.id,
                set_id: f.par_set_id.clone(),
                completed: false,
            });
        }
    }
    for cf in completed {
        if cf.par_file {
            par_files.push(ParCand {
                id: cf.id,
                set_id: cf.par_set_id.clone(),
                completed: true,
            });
        }
    }

    while !par_files.is_empty() {
        let set_id = par_files[0].set_id.clone();
        let mut best_idx: Option<usize> = None;
        let mut set_completed = false;

        let mut i = 0;
        while i < par_files.len() {
            if par_files[i].set_id.eq_ignore_ascii_case(&set_id) {
                set_completed |= par_files[i].completed;
                let pid = par_files[i].id;
                if let Some(fidx) = files.iter().position(|f| f.id == pid) {
                    if best_idx.is_none_or(|b| files[fidx].size < files[b].size) {
                        best_idx = Some(fidx);
                    }
                }
                par_files.remove(i);
            } else {
                i += 1;
            }
        }

        if let Some(b) = best_idx {
            if !set_completed {
                let bf = &files[b];
                let mut msg = b"Increasing priority for par2-file ".to_vec();
                msg.extend_from_slice(&bf.filename);
                host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
                host.file_set_int(bf.ptr, FILE_INT_PAUSED, 0);
                host.file_set_int(bf.ptr, FILE_INT_EXTRA_PRIORITY, 1);
            }
        }
    }
}

#[cfg(unix)]
fn start_par_loader_host(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb_ptr: usize,
) {
    use crate::queuesubsys_host::*;

    let nzb = host.nzb_snap(nzb_ptr, false);
    if nzb.skip_disk_write || opts.skip_write {
        return;
    }

    let mut msg = b"Directly checking renamed files for ".to_vec();
    msg.extend_from_slice(&nzb.name);
    host.nzb_print_message(nzb_ptr, MK_INFO, &msg);

    let completed = host.nzb_completed_files(nzb_ptr);
    let mut par_files = Vec::new();
    for cf in &completed {
        if cf.par_file {
            par_files.push(resolve_old_output_path(
                b"",
                &nzb.dest_dir,
                b'/',
                &cf.filename,
            ));
        }
    }

    coord.spawn_par_loader(host, nzb.id, par_files);
}

#[cfg(unix)]
pub(crate) fn run_par_loader(
    host: crate::queuesubsys_host::QueueSubsysHost,
    stopped: &std::sync::atomic::AtomicBool,
    nzb_id: i32,
    par_files: Vec<Vec<u8>>,
) {
    use crate::queuesubsys_host::*;

    host.log(MK_DEBUG, b"Started DirectParLoader");

    let mut par_hashes: Vec<(std::ffi::CString, std::ffi::CString)> = Vec::new();

    for par_file in &par_files {
        use std::os::unix::ffi::OsStrExt;
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(cut_nul(par_file)));
        match crate::parrenamer::load_par_entries_from_file(p) {
            None => {
                let mut w = b"Could not load par2-file ".to_vec();
                w.extend_from_slice(par_file);
                host.log(MK_WARNING, &w);
            }
            Some(entries) => {
                host.with_locked_queue(|dq_ptr| {
                    let nzb_ptr = host.queue_find_by_id(dq_ptr, nzb_id);
                    if nzb_ptr == 0 {
                        return;
                    }
                    let base = crate::queuecoord::base_file_name(par_file);
                    let mut info_msg = b"Loaded par2-file ".to_vec();
                    info_msg.extend_from_slice(base);
                    info_msg.extend_from_slice(b" for direct-rename");
                    host.nzb_print_message(nzb_ptr, MK_INFO, &info_msg);

                    for entry in entries {
                        if stopped.load(std::sync::atomic::Ordering::Acquire) {
                            break;
                        }
                        match entry {
                            crate::parrenamer::ParEntry::Damaged => {
                                let mut dmg = b"Damaged par2-file detected: ".to_vec();
                                dmg.extend_from_slice(base);
                                host.nzb_print_message(nzb_ptr, MK_WARNING, &dmg);
                                break;
                            }
                            crate::parrenamer::ParEntry::File { name, hash } => {
                                let n = cut_nul(&name);
                                let h = cut_nul(&hash);
                                let mut dbg = b"file: ".to_vec();
                                dbg.extend_from_slice(n);
                                dbg.extend_from_slice(b", hash-16k: ");
                                dbg.extend_from_slice(h);
                                host.log(MK_DEBUG, &dbg);
                                par_hashes.push((to_cstring(n), to_cstring(h)));
                            }
                        }
                    }
                });
            }
        }
    }

    host.with_locked_queue(|dq_ptr| {
        let opts = host.opts();
        let nzb_ptr = host.queue_find_by_id(dq_ptr, nzb_id);
        if nzb_ptr != 0 {
            let nzb = host.nzb_snap(nzb_ptr, false);
            if !nzb.skip_disk_write && !opts.skip_write {
                rename_files_host(&host, &opts, dq_ptr, nzb_ptr, &par_hashes);
            }
        }
    });
}

#[cfg(unix)]
fn rename_files_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb_ptr: usize,
    par_hashes: &[(std::ffi::CString, std::ffi::CString)],
) {
    use crate::queuesubsys_host::*;

    let mut vol: i32 = 1;
    let need_pars = check_need_rename_pars_host(host, nzb_ptr);

    let mut renamed_count =
        rename_files_in_progress_host(host, opts, nzb_ptr, par_hashes, need_pars, &mut vol);
    renamed_count +=
        rename_completed_files_host(host, opts, nzb_ptr, par_hashes, need_pars, &mut vol);

    let nzb = host.nzb_snap(nzb_ptr, false);
    if renamed_count > 0 {
        let mut msg = b"Successfully renamed ".to_vec();
        msg.extend_from_slice(renamed_count.to_string().as_bytes());
        msg.extend_from_slice(b" file(s) for ");
        msg.extend_from_slice(&nzb.name);
        host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
    } else {
        let mut msg = b"No renamed files found for ".to_vec();
        msg.extend_from_slice(&nzb.name);
        host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
    }

    crate::queuecoord::direct_rename_completed_host(host, opts, dq_ptr, nzb_ptr);
}

#[cfg(unix)]
fn check_need_rename_pars_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
) -> bool {
    let mut owned: Vec<(std::ffi::CString, std::ffi::CString)> = Vec::new();
    let flen = host.nzb_file_len(nzb_ptr);
    for i in 0..flen {
        let f = host.file_snap(host.nzb_file_at(nzb_ptr, i));
        if f.par_file {
            owned.push((to_cstring(&f.filename), to_cstring(&f.par_set_id)));
        }
    }
    for cf in host.nzb_completed_files(nzb_ptr) {
        if cf.par_file {
            owned.push((to_cstring(&cf.filename), to_cstring(&cf.par_set_id)));
        }
    }
    let refs: Vec<(Option<&CStr>, &CStr)> = owned
        .iter()
        .map(|(n, s)| (Some(n.as_c_str()), s.as_c_str()))
        .collect();
    need_rename_pars(&refs)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn resolve_new_name(
    filename: &[u8],
    par_file: bool,
    par_set_id: &[u8],
    hash16k: &[u8],
    dest_dir: &[u8],
    par_hashes: &[(std::ffi::CString, std::ffi::CString)],
    need_pars: bool,
    hard_linking: bool,
    vol: &mut i32,
) -> Vec<u8> {
    let mut new_name = if par_file && need_pars {
        par_name_on_disk(filename, dest_dir, par_set_id, b'/', vol)
    } else if !par_file {
        let c_old = to_cstring(filename);
        let c_hash = to_cstring(hash16k);
        match regular_name(
            &c_old,
            &c_hash,
            par_hashes.iter().map(|(f, h)| (f.as_c_str(), h.as_c_str())),
        ) {
            Some(idx) => par_hashes[idx].0.as_bytes().to_vec(),
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    if new_name.is_empty() && !par_file && hard_linking {
        new_name = filename.to_vec();
    }
    new_name
}

#[cfg(unix)]
fn try_create_hard_link_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
    file_ptr: usize,
    filename: &[u8],
    old_output_filename: &[u8],
    new_name: &[u8],
    phase_label: &[u8],
) {
    use crate::queuesubsys_host::*;

    if !crate::fsops::file_exists(old_output_filename, &crate::fsops::posix_stat) {
        let mut msg = b"Skipping hardlink for ".to_vec();
        msg.extend_from_slice(filename);
        msg.extend_from_slice(b": file not assembled yet");
        host.nzb_print_message(nzb_ptr, MK_DETAIL, &msg);
        return;
    }

    let nzb_final_dir = host.nzb_build_final_dir_name(nzb_ptr);
    let final_output_filename = resolve_old_output_path(b"", &nzb_final_dir, b'/', new_name);

    let mut info_msg = b"HardLinking ".to_vec();
    info_msg.extend_from_slice(phase_label);
    info_msg.extend_from_slice(b" file ");
    info_msg.extend_from_slice(filename);
    info_msg.extend_from_slice(b" to ");
    info_msg.extend_from_slice(new_name);
    host.nzb_print_message(nzb_ptr, MK_INFO, &info_msg);

    let c_old = to_cstring(old_output_filename);
    let c_final = to_cstring(&final_output_filename);
    let mut errmsg = Vec::new();
    if crate::fsops::create_hard_link(&c_old, &c_final, &crate::fsops::posix_stat, &mut errmsg) {
        if file_ptr != 0 {
            host.file_set_str(
                file_ptr,
                FILE_STR_HARD_LINK_PATH,
                Some(&final_output_filename),
            );
            host.nzb_set_str(nzb_ptr, NZB_STR_HARD_LINK_PATH, &nzb_final_dir);
        }
    } else {
        let mut err = b"Could not create hardlink ".to_vec();
        err.extend_from_slice(filename);
        err.extend_from_slice(b" to ");
        err.extend_from_slice(new_name);
        err.extend_from_slice(b": ");
        err.extend_from_slice(&errmsg);
        host.nzb_print_message(nzb_ptr, MK_ERROR, &err);
    }
}

#[cfg(unix)]
fn rename_files_in_progress_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb_ptr: usize,
    par_hashes: &[(std::ffi::CString, std::ffi::CString)],
    need_pars: bool,
    vol: &mut i32,
) -> i32 {
    use crate::queuesubsys_host::*;

    let mut renamed = 0;
    let nzb = host.nzb_snap(nzb_ptr, false);
    let flen = host.nzb_file_len(nzb_ptr);

    for i in 0..flen {
        let file_ptr = host.nzb_file_at(nzb_ptr, i);
        let f = host.file_snap(file_ptr);
        let new_name = resolve_new_name(
            &f.filename,
            f.par_file,
            &f.par_set_id,
            &f.hash16k,
            &nzb.dest_dir,
            par_hashes,
            need_pars,
            opts.hard_linking,
            vol,
        );
        if new_name.is_empty() {
            continue;
        }

        let old_output_filename =
            resolve_old_output_path(&f.output_filename, &nzb.dest_dir, b'/', &f.filename);

        let mut msg = b"Renaming in-progress file ".to_vec();
        msg.extend_from_slice(&f.filename);
        msg.extend_from_slice(b" to ");
        msg.extend_from_slice(&new_name);
        host.nzb_print_message(nzb_ptr, MK_INFO, &msg);

        if opts.hard_linking
            && !crate::util::match_file_ext_locale(&new_name, &opts.hard_linking_ignore_ext, b",")
        {
            try_create_hard_link_host(
                host,
                nzb_ptr,
                file_ptr,
                &f.filename,
                &old_output_filename,
                &new_name,
                b"in-progress",
            );
        }

        if f.origname.is_empty() {
            host.file_set_str(file_ptr, FILE_STR_ORIGNAME, Some(&f.filename));
        }
        host.file_set_str(file_ptr, FILE_STR_FILENAME, Some(&new_name));
        host.file_set_int(file_ptr, FILE_INT_FILENAME_CONFIRMED, 1);
        renamed += 1;
    }

    renamed
}

#[cfg(unix)]
fn rename_completed_files_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb_ptr: usize,
    par_hashes: &[(std::ffi::CString, std::ffi::CString)],
    need_pars: bool,
    vol: &mut i32,
) -> i32 {
    use crate::queuesubsys_host::*;

    let mut renamed = 0;
    let nzb = host.nzb_snap(nzb_ptr, false);
    let completed = host.nzb_completed_files(nzb_ptr);

    for (idx, cf) in completed.iter().enumerate() {
        let new_name = resolve_new_name(
            &cf.filename,
            cf.par_file,
            &cf.par_set_id,
            &cf.hash16k,
            &nzb.dest_dir,
            par_hashes,
            need_pars,
            opts.hard_linking,
            vol,
        );
        if new_name.is_empty() {
            continue;
        }

        let old_output_filename = resolve_old_output_path(b"", &nzb.dest_dir, b'/', &cf.filename);
        let output_filename = resolve_old_output_path(b"", &nzb.dest_dir, b'/', &new_name);

        let mut msg = b"Renaming completed file ".to_vec();
        msg.extend_from_slice(&cf.filename);
        msg.extend_from_slice(b" to ");
        msg.extend_from_slice(&new_name);
        host.nzb_print_message(nzb_ptr, MK_INFO, &msg);

        if !opts.inter_dir.is_empty()
            && opts.hard_linking
            && !crate::util::match_file_ext_locale(&new_name, &opts.hard_linking_ignore_ext, b",")
        {
            try_create_hard_link_host(
                host,
                nzb_ptr,
                0,
                &cf.filename,
                &old_output_filename,
                &new_name,
                b"completed",
            );
        }

        if rename_file_on_disk(host, nzb_ptr, &old_output_filename, &output_filename) {
            let orig = if cf.origname.is_empty() {
                Some(cf.filename.as_slice())
            } else {
                None
            };
            host.nzb_update_completed_file(nzb_ptr, idx, &new_name, orig);
            renamed += 1;
        }
    }

    renamed
}

#[cfg(unix)]
fn rename_file_on_disk(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
    old_full: &[u8],
    new_full: &[u8],
) -> bool {
    use crate::queuesubsys_host::*;

    let (old_path, old_name) = crate::paths::split_path_and_filename(old_full);
    let (new_path, new_name) = crate::paths::split_path_and_filename(new_full);

    if old_path != new_path {
        let mut errmsg = Vec::new();
        if !crate::fsops::force_directories(new_path, &mut errmsg, &crate::fsops::posix_stat) {
            let mut msg = b"Failed to rename file ".to_vec();
            msg.extend_from_slice(old_name);
            msg.extend_from_slice(b" to ");
            msg.extend_from_slice(new_name);
            msg.extend_from_slice(b". Could not create the target directory \"");
            msg.extend_from_slice(new_path);
            msg.extend_from_slice(b"\"");
            host.nzb_print_message(nzb_ptr, MK_ERROR, &msg);
            return false;
        }
    }

    let c_old = to_cstring(old_full);
    let c_new = to_cstring(new_full);
    if !crate::fsops::move_file(&c_old, &c_new) {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let sys_err = crate::fsops::error_message(errno);
        let mut msg = b"Failed to rename file ".to_vec();
        msg.extend_from_slice(old_name);
        msg.extend_from_slice(b" to ");
        msg.extend_from_slice(new_name);
        msg.extend_from_slice(b": ");
        msg.extend_from_slice(&sys_err);
        host.nzb_print_message(nzb_ptr, MK_ERROR, &msg);
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_name_takes_the_first_matching_hash() {
        let hashes = [(c"a.mkv", c"H1"), (c"b.mkv", c"H2"), (c"c.mkv", c"H2")];
        let find = |old: &CStr, hash: &CStr| regular_name(old, hash, hashes.iter().copied());
        assert_eq!(find(c"x.bin", c"H2"), Some(1));
        assert_eq!(find(c"B.MKV", c"H2"), None);
        assert_eq!(find(c"x.bin", c""), None);
        assert_eq!(find(c"x.bin", c"H3"), None);
    }

    #[test]
    fn par_name_reuses_the_suffix_or_counts_volumes() {
        let mut taken = vec![b"/d/S.vol01+02.par2".to_vec()];
        let mut vol = 1;
        let mut exists = |p: &[u8]| taken.iter().any(|t| t == p);
        assert_eq!(
            par_name(b"x.vol01+02.PaR2", b"/d", b"T", b'/', &mut vol, &mut exists),
            b"T.vol01+02.PaR2"
        );
        assert_eq!(vol, 1);
        assert_eq!(
            par_name(b"x.vol01+02.par2", b"/d", b"S", b'/', &mut vol, &mut exists),
            b"S.vol001+01.PAR2"
        );
        assert_eq!(vol, 2);
        taken.push(b"/d/S.vol002+01.PAR2".to_vec());
        let mut exists = |p: &[u8]| taken.iter().any(|t| t == p);
        assert_eq!(
            par_name(b"x.par2", b"/d", b"S", b'/', &mut vol, &mut exists),
            b"S.vol003+01.PAR2"
        );
        assert_eq!(vol, 4);
        assert_eq!(
            par_name(b"a.vol.b.vol1.par2", b"", b"S", b'/', &mut vol, &mut |_| {
                false
            }),
            b"S.vol.b.vol1.par2"
        );
        assert_eq!(
            par_name(b"a.vol1.par2.x", b"", b"S", b'/', &mut vol, &mut |_| false),
            b"S.vol004+01.PAR2"
        );
        let mut vol = -5;
        assert_eq!(
            par_name(b"", b"", b"S", b'/', &mut vol, &mut |_| false),
            b"S.vol0-5+01.PAR2"
        );
        let mut vol = 1234;
        assert_eq!(
            par_name(b"", b"", b"S", b'/', &mut vol, &mut |_| false),
            b"S.vol1234+01.PAR2"
        );
        let mut vol = i32::MAX;
        par_name(b"", b"", b"S", b'/', &mut vol, &mut |_| false);
        assert_eq!(vol, i32::MIN);
    }

    #[test]
    fn need_rename_pars_checks_extensions_and_sets() {
        let need = |pars: &[(Option<&CStr>, &CStr)]| need_rename_pars(pars);
        assert!(!need(&[]));
        assert!(!need(&[
            (Some(c"a.PAR2"), c"S"),
            (Some(c"A.vol1+2.par2"), c"S"),
            (Some(c"b.par2"), c"T")
        ]));
        assert!(need(&[
            (Some(c"a.par2"), c"S"),
            (Some(c"b.vol1+2.par2"), c"S")
        ]));
        assert!(need(&[(Some(c"a.par"), c"S")]));
        assert!(need(&[(None, c"S")]));
        assert!(need(&[(Some(c"a.par2"), c"S"), (None, c"S")]));
        assert!(need(&[(Some(c".par2"), c"S"), (Some(c".par2"), c"S")]));
        assert!(!need(&[(Some(c".par2"), c"S"), (Some(c".par2"), c"T")]));
    }
}
