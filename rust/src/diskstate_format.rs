//! DiskState record serialization and helpers (`daemon/queue/DiskState.cpp`).

use std::ffi::{c_char, c_int, c_void};
use std::io::Write;

use crate::ffi::input;

#[repr(C)]
pub struct NzbgetRsDiskCompletedFile {
    pub id: c_int,
    pub status: c_int,
    pub crc: u32,
    pub par_file: c_int,
    pub hash16k: *const c_char,
    pub par_set_id: *const c_char,
    pub filename: *const c_char,
    pub origname: *const c_char,
}

#[repr(C)]
pub struct NzbgetRsDiskParam {
    pub name: *const c_char,
    pub value: *const c_char,
}

#[repr(C)]
pub struct NzbgetRsDiskScriptStatus {
    pub status: c_int,
    pub name: *const c_char,
}

#[repr(C)]
pub struct NzbgetRsDiskServerStat {
    pub server_id: c_int,
    pub success_articles: c_int,
    pub failed_articles: c_int,
}

#[repr(C)]
pub struct NzbgetRsDiskFileStub {
    pub id: c_int,
    pub paused: c_int,
    pub extra_priority: c_int,
}

#[repr(C)]
pub struct NzbgetRsDiskNzbInfoInput {
    pub id: c_int,
    pub kind: c_int,
    pub url: *const c_char,
    pub filename: *const c_char,
    pub dest_dir: *const c_char,
    pub final_dir: *const c_char,
    pub hard_link_path: *const c_char,
    pub meta_name: *const c_char,
    pub queued_filename: *const c_char,
    pub name: *const c_char,
    pub category: *const c_char,
    pub priority: c_int,
    pub has_post_info: c_int,
    pub post_stage: c_int,
    pub delete_paused: c_int,
    pub many_dupe_files: c_int,
    pub feed_id: c_int,
    pub par_status: c_int,
    pub unpack_status: c_int,
    pub move_status: c_int,
    pub par_rename_status: c_int,
    pub rar_rename_status: c_int,
    pub direct_rename_status: c_int,
    pub delete_status: c_int,
    pub mark_status: c_int,
    pub url_status: c_int,
    pub unpack_cleaned_up_disk: c_int,
    pub health_paused: c_int,
    pub add_url_paused: c_int,
    pub file_count: c_int,
    pub parked_file_count: c_int,
    pub message_count: c_int,
    pub min_time: c_int,
    pub max_time: c_int,
    pub par_full: c_int,
    pub force_par_full: c_int,
    pub force_repair: c_int,
    pub extra_par_blocks: c_int,
    pub full_content_hash: u32,
    pub filtered_content_hash: u32,
    pub size: i64,
    pub success_size: i64,
    pub failed_size: i64,
    pub par_size: i64,
    pub par_success_size: i64,
    pub par_failed_size: i64,
    pub total_articles: c_int,
    pub success_articles: c_int,
    pub failed_articles: c_int,
    pub dupe_key: *const c_char,
    pub dupe_mode: c_int,
    pub dupe_score: c_int,
    pub dupe_hint: c_int,
    pub downloaded_size: i64,
    pub download_sec: c_int,
    pub post_total_sec: c_int,
    pub par_sec: c_int,
    pub repair_sec: c_int,
    pub unpack_sec: c_int,
    pub skip_script_processing: c_int,
    pub skip_disk_write: c_int,
    pub desired_server_id: c_int,
}

#[repr(C)]
pub struct NzbgetRsDiskArticleInfo {
    pub part_number: c_int,
    pub size: c_int,
    pub message_id: *const c_char,
}

#[repr(C)]
pub struct NzbgetRsDiskFileInfoInput {
    pub subject: *const c_char,
    pub filename: *const c_char,
    pub origname: *const c_char,
    pub output_filename: *const c_char,
    pub filename_confirmed: c_int,
    pub time: c_int,
    pub output_initialized: c_int,
    pub size: i64,
    pub missed_size: i64,
    pub par_file: c_int,
    pub total_articles: c_int,
    pub missed_articles: c_int,
}

#[repr(C)]
pub struct NzbgetRsDiskArticleState {
    pub saved_status: c_int,
    pub segment_offset: i64,
    pub segment_size: c_int,
    pub crc: u32,
    pub staged_fallback: c_int,
}

#[repr(C)]
pub struct NzbgetRsDiskFileStateInput {
    pub success_articles: c_int,
    pub failed_articles: c_int,
    pub remaining_size: i64,
    pub success_size: i64,
    pub failed_size: i64,
    pub filename: *const c_char,
    pub hash16k: *const c_char,
    pub par_set_id: *const c_char,
    pub par_file: c_int,
    pub decoded_file_size: i64,
    pub output_initialized: c_int,
}

#[repr(C)]
pub struct NzbgetRsDiskStreamHole {
    pub offset: i64,
    pub size: i64,
}

#[repr(C)]
pub struct NzbgetRsDiskRepairJob {
    pub file_id: c_int,
    pub decoded_file_size: i64,
    pub failed_size: i64,
    pub missed_size: i64,
    pub par_file: c_int,
    pub failed_articles: c_int,
    pub steps_hash: u64,
    pub filename: *const c_char,
    pub holes: *const NzbgetRsDiskStreamHole,
    pub holes_len: usize,
}

#[repr(C)]
pub struct NzbgetRsDiskVolumeStatInput {
    pub first_day: c_int,
    pub data_time: c_int,
    pub custom_time: c_int,
    pub counters_reset_time: c_int,
    pub total_bytes: i64,
    pub custom_bytes: i64,
    pub bytes_per_seconds: *const i64,
    pub bytes_per_seconds_len: usize,
    pub bytes_per_minutes: *const i64,
    pub bytes_per_minutes_len: usize,
    pub bytes_per_hours: *const i64,
    pub bytes_per_hours_len: usize,
    pub bytes_per_days: *const i64,
    pub bytes_per_days_len: usize,
    pub articles_per_days: *const crate::xmlrpc_format::NzbgetRsRpcArticlePair,
    pub articles_per_days_len: usize,
}

#[inline]
fn split_i64(val: i64) -> (u32, u32) {
    let u = val as u64;
    ((u >> 32) as u32, u as u32)
}

#[inline]
fn write_sanitized_bytes(raw: &[u8], out: &mut Vec<u8>) {
    for &b in raw {
        if b == b'\n' || b == b'\r' {
            out.push(b' ');
        } else {
            out.push(b);
        }
    }
}

#[inline]
fn write_sanitized_line(raw: &[u8], out: &mut Vec<u8>) {
    write_sanitized_bytes(raw, out);
    out.push(b'\n');
}

/// # Safety
/// `inp` and all slice pointers must be valid for the call duration.
#[allow(clippy::too_many_arguments)]
pub unsafe fn format_nzb_info_bytes(
    inp: *const NzbgetRsDiskNzbInfoInput,
    completed: *const NzbgetRsDiskCompletedFile,
    completed_len: usize,
    params: *const NzbgetRsDiskParam,
    params_len: usize,
    scripts: *const NzbgetRsDiskScriptStatus,
    scripts_len: usize,
    stats: *const NzbgetRsDiskServerStat,
    stats_len: usize,
    files: *const NzbgetRsDiskFileStub,
    files_len: usize,
) -> Vec<u8> {
    if inp.is_null() {
        return Vec::new();
    }
    let n = unsafe { &*inp };
    let mut out = Vec::with_capacity(2048);

    let _ = writeln!(out, "{}", n.id);
    let _ = writeln!(out, "{}", n.kind);
    write_sanitized_line(unsafe { input(n.url) }, &mut out);
    write_sanitized_line(unsafe { input(n.filename) }, &mut out);
    write_sanitized_line(unsafe { input(n.dest_dir) }, &mut out);
    write_sanitized_line(unsafe { input(n.final_dir) }, &mut out);
    write_sanitized_line(unsafe { input(n.hard_link_path) }, &mut out);
    write_sanitized_line(unsafe { input(n.meta_name) }, &mut out);
    write_sanitized_line(unsafe { input(n.queued_filename) }, &mut out);
    write_sanitized_line(unsafe { input(n.name) }, &mut out);
    write_sanitized_line(unsafe { input(n.category) }, &mut out);

    let persisted_stage = if n.has_post_info != 0 {
        n.post_stage + 1
    } else {
        0
    };
    let _ = writeln!(
        out,
        "{},{},{},{},{}",
        n.priority, persisted_stage, n.delete_paused, n.many_dupe_files, n.feed_id
    );
    let _ = writeln!(
        out,
        "{},{},{},{},{},{},{},{},{}",
        n.par_status,
        n.unpack_status,
        n.move_status,
        n.par_rename_status,
        n.rar_rename_status,
        n.direct_rename_status,
        n.delete_status,
        n.mark_status,
        n.url_status
    );
    let _ = writeln!(
        out,
        "{},{},{}",
        n.unpack_cleaned_up_disk, n.health_paused, n.add_url_paused
    );
    let _ = writeln!(
        out,
        "{},{},{}",
        n.file_count, n.parked_file_count, n.message_count
    );
    let _ = writeln!(out, "{},{}", n.min_time, n.max_time);
    let _ = writeln!(
        out,
        "{},{},{},{}",
        n.par_full, n.force_par_full, n.force_repair, n.extra_par_blocks
    );
    let _ = writeln!(out, "{},{}", n.full_content_hash, n.filtered_content_hash);

    let (sz_hi, sz_lo) = split_i64(n.size);
    let (suc_hi, suc_lo) = split_i64(n.success_size);
    let (fail_hi, fail_lo) = split_i64(n.failed_size);
    let _ = writeln!(
        out,
        "{},{},{},{},{},{}",
        sz_hi, sz_lo, suc_hi, suc_lo, fail_hi, fail_lo
    );

    let (psz_hi, psz_lo) = split_i64(n.par_size);
    let (psuc_hi, psuc_lo) = split_i64(n.par_success_size);
    let (pfail_hi, pfail_lo) = split_i64(n.par_failed_size);
    let _ = writeln!(
        out,
        "{},{},{},{},{},{}",
        psz_hi, psz_lo, psuc_hi, psuc_lo, pfail_hi, pfail_lo
    );

    let _ = writeln!(
        out,
        "{},{},{}",
        n.total_articles, n.success_articles, n.failed_articles
    );
    write_sanitized_line(unsafe { input(n.dupe_key) }, &mut out);
    let _ = writeln!(out, "{},{},{}", n.dupe_mode, n.dupe_score, n.dupe_hint);

    let (dl_hi, dl_lo) = split_i64(n.downloaded_size);
    let _ = writeln!(
        out,
        "{},{},{},{},{},{},{}",
        dl_hi,
        dl_lo,
        n.download_sec,
        n.post_total_sec,
        n.par_sec,
        n.repair_sec,
        n.unpack_sec
    );

    let comp_slice = if completed.is_null() || completed_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(completed, completed_len) }
    };
    let _ = writeln!(out, "{}", comp_slice.len() as c_int);
    for cf in comp_slice {
        let _ = write!(out, "{},{},{},{},", cf.id, cf.status, cf.crc, cf.par_file);
        write_sanitized_bytes(unsafe { input(cf.hash16k) }, &mut out);
        out.push(b',');
        write_sanitized_line(unsafe { input(cf.par_set_id) }, &mut out);
        write_sanitized_line(unsafe { input(cf.filename) }, &mut out);
        write_sanitized_line(unsafe { input(cf.origname) }, &mut out);
    }

    let param_slice = if params.is_null() || params_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(params, params_len) }
    };
    let _ = writeln!(
        out,
        "{},{},{},{}",
        param_slice.len() as c_int,
        n.skip_script_processing,
        n.skip_disk_write,
        n.desired_server_id
    );
    for p in param_slice {
        write_sanitized_bytes(unsafe { input(p.name) }, &mut out);
        out.push(b'=');
        write_sanitized_line(unsafe { input(p.value) }, &mut out);
    }

    let script_slice = if scripts.is_null() || scripts_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(scripts, scripts_len) }
    };
    let _ = writeln!(out, "{}", script_slice.len() as c_int);
    for sc in script_slice {
        let _ = write!(out, "{},", sc.status);
        write_sanitized_line(unsafe { input(sc.name) }, &mut out);
    }

    let stat_slice = if stats.is_null() || stats_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(stats, stats_len) }
    };
    let _ = writeln!(out, "{}", stat_slice.len() as c_int);
    for st in stat_slice {
        let _ = writeln!(
            out,
            "{},{},{}",
            st.server_id, st.success_articles, st.failed_articles
        );
    }

    let file_slice = if files.is_null() || files_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(files, files_len) }
    };
    let _ = writeln!(out, "{}", file_slice.len() as c_int);
    for f in file_slice {
        let _ = writeln!(out, "{},{},{}", f.id, f.paused, f.extra_priority);
    }

    out
}

/// # Safety
/// `jobs` must be valid for `jobs_len` items.
pub unsafe fn format_dupe_extras_bytes(
    recovered_articles: c_int,
    recovered_bytes: i64,
    recovered_holes: c_int,
    jobs: *const NzbgetRsDiskRepairJob,
    jobs_len: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    let (rec_hi, rec_lo) = split_i64(recovered_bytes);
    let _ = writeln!(
        out,
        "{},{},{},{}",
        recovered_articles, rec_hi, rec_lo, recovered_holes
    );

    let job_slice = if jobs.is_null() || jobs_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(jobs, jobs_len) }
    };
    let _ = writeln!(out, "{}", job_slice.len() as c_int);
    for job in job_slice {
        let (dec_hi, dec_lo) = split_i64(job.decoded_file_size);
        let (fail_hi, fail_lo) = split_i64(job.failed_size);
        let (miss_hi, miss_lo) = split_i64(job.missed_size);
        let (steps_hi, steps_lo) = split_i64(job.steps_hash as i64);
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{}",
            job.file_id,
            dec_hi,
            dec_lo,
            fail_hi,
            fail_lo,
            miss_hi,
            miss_lo,
            job.par_file,
            job.failed_articles,
            steps_hi,
            steps_lo
        );
        write_sanitized_line(unsafe { input(job.filename) }, &mut out);
        let hole_slice = if job.holes.is_null() || job.holes_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(job.holes, job.holes_len) }
        };
        let _ = writeln!(out, "{}", hole_slice.len() as c_int);
        for hole in hole_slice {
            let (off_hi, off_lo) = split_i64(hole.offset);
            let (sz_hi, sz_lo) = split_i64(hole.size);
            let _ = writeln!(out, "{},{},{},{}", off_hi, off_lo, sz_hi, sz_lo);
        }
    }
    out
}

/// # Safety
/// `inp`, `groups`, and `articles` must be valid for the call duration.
pub unsafe fn format_file_info_bytes(
    inp: *const NzbgetRsDiskFileInfoInput,
    groups: *const *const c_char,
    groups_len: usize,
    include_articles: c_int,
    articles: *const NzbgetRsDiskArticleInfo,
    articles_len: usize,
) -> Vec<u8> {
    if inp.is_null() {
        return Vec::new();
    }
    let f = unsafe { &*inp };
    let mut out = Vec::with_capacity(articles_len * 64 + 512);

    write_sanitized_line(unsafe { input(f.subject) }, &mut out);
    write_sanitized_line(unsafe { input(f.filename) }, &mut out);
    write_sanitized_line(unsafe { input(f.origname) }, &mut out);
    write_sanitized_line(unsafe { input(f.output_filename) }, &mut out);

    let _ = writeln!(
        out,
        "{},{},{}",
        f.filename_confirmed, f.time, f.output_initialized
    );

    let (sz_hi, sz_lo) = split_i64(f.size);
    let _ = writeln!(out, "{},{}", sz_hi, sz_lo);

    let (ms_hi, ms_lo) = split_i64(f.missed_size);
    let _ = writeln!(out, "{},{}", ms_hi, ms_lo);

    let _ = writeln!(out, "{}", f.par_file);
    let _ = writeln!(out, "{},{}", f.total_articles, f.missed_articles);

    let grp_slice = if groups.is_null() || groups_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(groups, groups_len) }
    };
    let _ = writeln!(out, "{}", grp_slice.len() as c_int);
    for &grp_ptr in grp_slice {
        write_sanitized_line(unsafe { input(grp_ptr) }, &mut out);
    }

    if include_articles != 0 {
        let art_slice = if articles.is_null() || articles_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(articles, articles_len) }
        };
        let _ = writeln!(out, "{}", art_slice.len() as c_int);
        for art in art_slice {
            let _ = writeln!(out, "{},{}", art.part_number, art.size);
            write_sanitized_line(unsafe { input(art.message_id) }, &mut out);
        }
    }

    out
}

/// # Safety
/// `inp`, `stats`, and `articles` must be valid for the call duration.
pub unsafe fn format_file_state_bytes(
    inp: *const NzbgetRsDiskFileStateInput,
    stats: *const NzbgetRsDiskServerStat,
    stats_len: usize,
    articles: *const NzbgetRsDiskArticleState,
    articles_len: usize,
) -> Vec<u8> {
    if inp.is_null() {
        return Vec::new();
    }
    let f = unsafe { &*inp };
    let mut out = Vec::with_capacity(articles_len * 40 + 512);

    let _ = writeln!(out, "{},{}", f.success_articles, f.failed_articles);

    let (rem_hi, rem_lo) = split_i64(f.remaining_size);
    let (suc_hi, suc_lo) = split_i64(f.success_size);
    let (fail_hi, fail_lo) = split_i64(f.failed_size);
    let _ = writeln!(
        out,
        "{},{},{},{},{},{}",
        rem_hi, rem_lo, suc_hi, suc_lo, fail_hi, fail_lo
    );

    write_sanitized_line(unsafe { input(f.filename) }, &mut out);
    write_sanitized_line(unsafe { input(f.hash16k) }, &mut out);
    write_sanitized_line(unsafe { input(f.par_set_id) }, &mut out);
    let _ = writeln!(out, "{}", f.par_file);

    let stat_slice = if stats.is_null() || stats_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(stats, stats_len) }
    };
    let _ = writeln!(out, "{}", stat_slice.len() as c_int);
    for st in stat_slice {
        let _ = writeln!(
            out,
            "{},{},{}",
            st.server_id, st.success_articles, st.failed_articles
        );
    }

    let art_slice = if articles.is_null() || articles_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(articles, articles_len) }
    };
    let _ = writeln!(out, "{}", art_slice.len() as c_int);
    for art in art_slice {
        let _ = writeln!(
            out,
            "{},{},{},{},{}",
            art.saved_status,
            art.segment_offset,
            art.segment_size,
            art.crc,
            art.staged_fallback
        );
    }

    let (dec_hi, dec_lo) = split_i64(f.decoded_file_size);
    let _ = writeln!(out, "#decoded {},{}", dec_hi, dec_lo);
    let _ = writeln!(out, "OutputInitialized={}", f.output_initialized);

    out
}

/// # Safety
/// `name` and `dupe_key` are null or valid NUL-terminated strings.
#[allow(clippy::too_many_arguments)]
pub unsafe fn format_dup_info_bytes(
    status: c_int,
    size: i64,
    full_content_hash: u32,
    filtered_content_hash: u32,
    dupe_score: c_int,
    dupe_mode: c_int,
    name: *const c_char,
    dupe_key: *const c_char,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    let (sz_hi, sz_lo) = split_i64(size);
    let _ = writeln!(
        out,
        "{},{},{},{},{},{},{}",
        status,
        sz_hi,
        sz_lo,
        full_content_hash,
        filtered_content_hash,
        dupe_score,
        dupe_mode
    );
    write_sanitized_line(unsafe { input(name) }, &mut out);
    write_sanitized_line(unsafe { input(dupe_key) }, &mut out);
    out
}

/// # Safety
/// `inp` and all slice pointers must be valid for the call duration.
pub unsafe fn format_volume_stat_item_bytes(inp: *const NzbgetRsDiskVolumeStatInput) -> Vec<u8> {
    if inp.is_null() {
        return Vec::new();
    }
    let v = unsafe { &*inp };
    let mut out = Vec::with_capacity(2048);

    let _ = writeln!(
        out,
        "{},{},{},{}",
        v.first_day, v.data_time, v.custom_time, v.counters_reset_time
    );

    let (tot_hi, tot_lo) = split_i64(v.total_bytes);
    let (cus_hi, cus_lo) = split_i64(v.custom_bytes);
    let _ = writeln!(out, "{},{},{},{}", tot_hi, tot_lo, cus_hi, cus_lo);

    let arrays = [
        (v.bytes_per_seconds, v.bytes_per_seconds_len),
        (v.bytes_per_minutes, v.bytes_per_minutes_len),
        (v.bytes_per_hours, v.bytes_per_hours_len),
        (v.bytes_per_days, v.bytes_per_days_len),
    ];
    for (ptr, len) in arrays {
        let slice = if ptr.is_null() || len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(ptr, len) }
        };
        let _ = writeln!(out, "{}", slice.len() as c_int);
        for &bytes in slice {
            let (hi, lo) = split_i64(bytes);
            let _ = writeln!(out, "{},{}", hi, lo);
        }
    }

    let art_slice = if v.articles_per_days.is_null() || v.articles_per_days_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(v.articles_per_days, v.articles_per_days_len) }
    };
    let art_count = u32::try_from(art_slice.len()).unwrap_or(0);
    let _ = writeln!(out, "{}", art_count);
    for art in art_slice {
        let _ = writeln!(out, "{},{}", art.failed, art.success);
    }

    out
}

pub unsafe fn format_unpack_trailer_item_bytes(
    id: c_int,
    direct_unpack_status: c_int,
    unpack_tried: c_int,
    par_repaired: c_int,
    last_unpack_status: c_int,
    archives: *const *const c_char,
    archives_len: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    let unpack_tried_flags = pack_unpack_tried_flags(
        unpack_tried != 0,
        par_repaired != 0,
    );
    let _ = writeln!(out, "{}", id);
    let _ = writeln!(out, "DirectUnpackStatus={}", direct_unpack_status);
    let _ = writeln!(out, "UnpackTried={}", unpack_tried_flags);
    let _ = writeln!(out, "LastUnpackStatus={}", last_unpack_status);
    out.extend_from_slice(b"ExtractedArchives=");
    let slice = if archives.is_null() || archives_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(archives, archives_len) }
    };
    for (idx, &arc_ptr) in slice.iter().enumerate() {
        if idx > 0 {
            out.push(b',');
        }
        write_sanitized_bytes(unsafe { input(arc_ptr) }, &mut out);
    }
    out.push(b'\n');
    out
}

pub unsafe fn format_feed_status_bytes(
    urls: *const *const c_char,
    filter_hashes: *const u32,
    last_updates: *const c_int,
    len: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(len.saturating_mul(64) + 16);
    let _ = writeln!(out, "{}", len as c_int);
    if !urls.is_null() && !filter_hashes.is_null() && !last_updates.is_null() && len > 0 {
        let u_slice = unsafe { std::slice::from_raw_parts(urls, len) };
        let h_slice = unsafe { std::slice::from_raw_parts(filter_hashes, len) };
        let t_slice = unsafe { std::slice::from_raw_parts(last_updates, len) };
        for i in 0..len {
            write_sanitized_line(unsafe { input(u_slice[i]) }, &mut out);
            let _ = writeln!(out, "{}", h_slice[i]);
            let _ = writeln!(out, "{}", t_slice[i]);
        }
    }
    out
}

pub unsafe fn format_feed_history_bytes(
    statuses: *const c_int,
    last_seens: *const c_int,
    urls: *const *const c_char,
    len: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(len.saturating_mul(64) + 16);
    let _ = writeln!(out, "{}", len as c_int);
    if !statuses.is_null() && !last_seens.is_null() && !urls.is_null() && len > 0 {
        let s_slice = unsafe { std::slice::from_raw_parts(statuses, len) };
        let t_slice = unsafe { std::slice::from_raw_parts(last_seens, len) };
        let u_slice = unsafe { std::slice::from_raw_parts(urls, len) };
        for i in 0..len {
            let _ = writeln!(out, "{},{}", s_slice[i], t_slice[i]);
            write_sanitized_line(unsafe { input(u_slice[i]) }, &mut out);
        }
    }
    out
}

pub unsafe fn format_server_info_bytes(
    names: *const *const c_char,
    hosts: *const *const c_char,
    ports: *const c_int,
    users: *const *const c_char,
    len: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(len.saturating_mul(64) + 16);
    let _ = writeln!(out, "{}", len as c_int);
    if !names.is_null() && !hosts.is_null() && !ports.is_null() && !users.is_null() && len > 0 {
        let n_slice = unsafe { std::slice::from_raw_parts(names, len) };
        let h_slice = unsafe { std::slice::from_raw_parts(hosts, len) };
        let p_slice = unsafe { std::slice::from_raw_parts(ports, len) };
        let u_slice = unsafe { std::slice::from_raw_parts(users, len) };
        for i in 0..len {
            write_sanitized_line(unsafe { input(n_slice[i]) }, &mut out);
            write_sanitized_line(unsafe { input(h_slice[i]) }, &mut out);
            let _ = writeln!(out, "{}", p_slice[i]);
            write_sanitized_line(unsafe { input(u_slice[i]) }, &mut out);
        }
    }
    out
}

pub unsafe fn format_nzb_message_line_bytes(
    time_str: *const c_char,
    tm: c_int,
    kind: c_int,
    text: *const c_char,
    line_ending: *const c_char,
) -> Vec<u8> {
    const MESSAGE_TYPES: [&[u8]; 5] = [b"INFO", b"WARNING", b"ERROR", b"DEBUG", b"DETAIL"];
    let kind_bytes = if (0..5).contains(&kind) {
        MESSAGE_TYPES[kind as usize]
    } else {
        b"INFO"
    };
    let mut clean_text = unsafe { input(text) }.to_vec();
    if clean_text.len() > 1023 {
        clean_text.truncate(1023);
    }
    sanitize_log_text_in_place(&mut clean_text);
    let mut out = Vec::with_capacity(clean_text.len() + 64);
    out.extend_from_slice(unsafe { input(time_str) });
    out.push(b'\t');
    let _ = write!(out, "{}", tm as u32);
    out.push(b'\t');
    out.extend_from_slice(kind_bytes);
    out.push(b'\t');
    out.extend_from_slice(&clean_text);
    out.extend_from_slice(unsafe { input(line_ending) });
    out
}

fn parse_c_int_field(field: &[u8]) -> Option<c_int> {
    let s = std::str::from_utf8(field).ok()?.trim_start();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        c_int::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<c_int>().ok()
    }
}

fn parse_u32_field(field: &[u8]) -> Option<u32> {
    let s = std::str::from_utf8(field).ok()?.trim_start();
    s.parse::<u32>().ok()
}

/// Parses a completed-file line in `DiskState::LoadNzbInfo` (mutating `buf` in place
/// for `format_version >= 60` comma-separated `hash16k` and `parSetId` fields).
/// Returns `1` on success, `0` on parse error.
#[allow(clippy::too_many_arguments)]
pub unsafe fn parse_completed_file_line(
    buf: *mut c_char,
    format_version: c_int,
    out_id: *mut c_int,
    out_status: *mut c_int,
    out_crc: *mut u32,
    out_par_file: *mut c_int,
    out_hash16k: *mut *const c_char,
    out_par_set_id: *mut *const c_char,
    out_file_name: *mut *const c_char,
) -> c_int {
    if buf.is_null() {
        return 0;
    }
    let mut id: c_int = 0;
    let mut status: c_int = 0;
    let mut crc: u32 = 0;
    let mut par_file: c_int = 0;
    let mut hash16k: *const c_char = std::ptr::null();
    let mut par_set_id: *const c_char = std::ptr::null();
    let mut file_name: *const c_char = buf;

    let len = unsafe { std::ffi::CStr::from_ptr(buf).to_bytes().len() };
    let bytes = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), len) };

    if format_version >= 49 {
        if format_version >= 60 {
            let mut commas = [0usize; 6];
            let mut c_count = 0usize;
            for (idx, &b) in bytes.iter().enumerate() {
                if b == b',' && c_count < 6 {
                    commas[c_count] = idx;
                    c_count += 1;
                }
            }
            if c_count < 3 {
                return 0;
            }
            let f0 = &bytes[..commas[0]];
            let f1 = &bytes[commas[0] + 1..commas[1]];
            let f2 = &bytes[commas[1] + 1..commas[2]];
            let f3_end = if c_count >= 4 { commas[3] } else { len };
            let f3 = &bytes[commas[2] + 1..f3_end];
            let (Some(p_id), Some(p_status), Some(p_crc), Some(p_par)) = (
                parse_c_int_field(f0),
                parse_c_int_field(f1),
                parse_u32_field(f2),
                parse_c_int_field(f3),
            ) else {
                return 0;
            };
            id = p_id;
            status = p_status;
            crc = p_crc;
            par_file = p_par;
            if c_count >= 4 {
                hash16k = unsafe { buf.add(commas[3] + 1) };
                if c_count >= 5 {
                    bytes[commas[4]] = 0;
                    par_set_id = unsafe { buf.add(commas[4] + 1) };
                    if c_count >= 6 {
                        bytes[commas[5]] = 0;
                        file_name = unsafe { buf.add(commas[5] + 1) };
                    } else {
                        file_name = std::ptr::null();
                    }
                } else {
                    file_name = unsafe { buf.add(1) };
                }
            } else {
                file_name = unsafe { buf.add(1) };
            }
        } else if format_version >= 50 {
            let mut commas = [0usize; 3];
            let mut c_count = 0usize;
            for (idx, &b) in bytes.iter().enumerate() {
                if b == b',' && c_count < 3 {
                    commas[c_count] = idx;
                    c_count += 1;
                }
            }
            if c_count < 2 {
                return 0;
            }
            let f0 = &bytes[..commas[0]];
            let f1 = &bytes[commas[0] + 1..commas[1]];
            let f2_end = if c_count >= 3 { commas[2] } else { len };
            let f2 = &bytes[commas[1] + 1..f2_end];
            let (Some(p_id), Some(p_status), Some(p_crc)) = (
                parse_c_int_field(f0),
                parse_c_int_field(f1),
                parse_u32_field(f2),
            ) else {
                return 0;
            };
            id = p_id;
            status = p_status;
            crc = p_crc;
            file_name = if c_count >= 3 {
                unsafe { buf.add(commas[2] + 1) }
            } else {
                std::ptr::null()
            };
        } else {
            let Some(c0) = bytes.iter().position(|&b| b == b',') else {
                return 0;
            };
            let rest = &bytes[c0 + 1..];
            let f1_end = rest.iter().position(|&b| b == b',').unwrap_or(rest.len());
            let (Some(p_status), Some(p_crc)) = (
                parse_c_int_field(&bytes[..c0]),
                parse_u32_field(&rest[..f1_end]),
            ) else {
                return 0;
            };
            status = p_status;
            crc = p_crc;
            let search_start = 2.min(len);
            file_name = match bytes[search_start..].iter().position(|&b| b == b',') {
                Some(pos) => unsafe { buf.add(search_start + pos + 1) },
                None => std::ptr::null(),
            };
        }
    }

    if !out_id.is_null() {
        unsafe { *out_id = id };
    }
    if !out_status.is_null() {
        unsafe { *out_status = status };
    }
    if !out_crc.is_null() {
        unsafe { *out_crc = crc };
    }
    if !out_par_file.is_null() {
        unsafe { *out_par_file = par_file };
    }
    if !out_hash16k.is_null() {
        unsafe { *out_hash16k = hash16k };
    }
    if !out_par_set_id.is_null() {
        unsafe { *out_par_set_id = par_set_id };
    }
    if !out_file_name.is_null() {
        unsafe { *out_file_name = file_name };
    }
    1
}

/// Formats `"<base_path>.new"` for `StateFile::StateFile` and `DiskState::SaveIdFloor`.
pub fn diskstate_format_new_path(base_path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(base_path.len() + 4);
    out.extend_from_slice(base_path);
    out.extend_from_slice(b".new");
    out
}

/// Formats `"nzb <nzb_id> file <file_id>\n"` for `DiskState::SaveIdFloor`.
pub fn diskstate_format_ids_line(nzb_id: i32, file_id: i32) -> Vec<u8> {
    format!("nzb {nzb_id} file {file_id}\n").into_bytes()
}

/// Formats `"<temp_dir><sep><file_id>.<part_num:03>"` for `DiskState::LoadFileState`.
pub fn diskstate_format_staged_part_path(
    temp_dir: &[u8],
    sep: u8,
    file_id: i32,
    part_num: i32,
) -> Vec<u8> {
    let suffix = if part_num < 0 {
        format!("{file_id}.-{:03}", part_num.unsigned_abs())
    } else {
        format!("{file_id}.{part_num:03}")
    };
    let mut out = Vec::with_capacity(temp_dir.len() + 1 + suffix.len());
    out.extend_from_slice(temp_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"<file_id>c"` or `"<file_id>s"` for `DiskState::SaveFileState` and `DiskState::LoadFileState`.
pub fn diskstate_format_file_state_name(file_id: i32, completed: bool) -> Vec<u8> {
    let ch = if completed { 'c' } else { 's' };
    format!("{file_id}{ch}").into_bytes()
}

/// Parses `(id, suffix)` from a queue state filename following `sscanf(filename, "%i%c", &id, &suffix) == 2`
/// semantics in `DiskState::LoadAllFileStates`.
pub fn diskstate_parse_queue_file_id_and_suffix(filename: &[u8]) -> Option<(i32, u8)> {
    let mut i = 0;
    while i < filename.len() && matches!(filename[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < filename.len() {
        if filename[i] == b'-' {
            neg = true;
            i += 1;
        } else if filename[i] == b'+' {
            i += 1;
        }
    }
    if i >= filename.len() {
        return None;
    }
    let mut acc: u64 = 0;
    let mut any = false;
    if filename[i] == b'0' {
        if i + 2 < filename.len()
            && (filename[i + 1] == b'x' || filename[i + 1] == b'X')
            && filename[i + 2].is_ascii_hexdigit()
        {
            i += 2;
            while i < filename.len() && filename[i].is_ascii_hexdigit() {
                any = true;
                let d = match filename[i] {
                    b'0'..=b'9' => filename[i] - b'0',
                    b'a'..=b'f' => filename[i] - b'a' + 10,
                    b'A'..=b'F' => filename[i] - b'A' + 10,
                    _ => unreachable!(),
                };
                acc = acc.saturating_mul(16).saturating_add(u64::from(d));
                i += 1;
            }
        } else {
            while i < filename.len() && matches!(filename[i], b'0'..=b'7') {
                any = true;
                acc = acc
                    .saturating_mul(8)
                    .saturating_add(u64::from(filename[i] - b'0'));
                i += 1;
            }
        }
    } else {
        while i < filename.len() && filename[i].is_ascii_digit() {
            any = true;
            acc = acc
                .saturating_mul(10)
                .saturating_add(u64::from(filename[i] - b'0'));
            i += 1;
        }
    }
    if !any || i >= filename.len() {
        return None;
    }
    let val = if neg {
        let max_neg = (i64::MAX as u64) + 1;
        if acc >= max_neg {
            i64::MIN
        } else {
            -(acc as i64)
        }
    } else {
        acc.min(i64::MAX as u64) as i64
    };
    Some((val as i32, filename[i]))
}

/// Finds the next non-empty comma-delimited token `(tok_start, tok_len, next_cursor)` in `s`
/// starting from `cursor`, for `DiskState::LoadDupeRepairState` and `DiskState::LoadNzbInfo`.
pub fn diskstate_next_comma_token(
    s: &[u8],
    cursor: usize,
) -> Option<(usize, usize, usize)> {
    let mut pos = cursor;
    while pos < s.len() {
        while pos < s.len() && s[pos] == b',' {
            pos += 1;
        }
        if pos >= s.len() {
            return None;
        }
        let end = pos
            + s[pos..]
                .iter()
                .take_while(|&&b| b != b',')
                .count();
        let next = if end < s.len() { end + 1 } else { end };
        if end > pos {
            return Some((pos, end - pos, next));
        }
        pos = next;
    }
    None
}

/// Selects `orig_msg_id` when non-empty, otherwise `cur_msg_id` (`DiskState::SaveFileInfo` and `DupeArticleFallback::NzbFilenameOf`).
pub fn diskstate_select_original_or_current_msg_id<'a>(
    orig_msg_id: &'a [u8],
    cur_msg_id: &'a [u8],
) -> &'a [u8] {
    if !orig_msg_id.is_empty() {
        orig_msg_id
    } else {
        cur_msg_id
    }
}

/// Convert a C string pointer safely to a byte slice.
pub unsafe fn cstr_to_bytes<'a>(ptr: *const c_char) -> &'a [u8] {
    if ptr.is_null() {
        return &[];
    }
    unsafe { std::ffi::CStr::from_ptr(ptr).to_bytes() }
}

#[inline]
fn ascii_lower(b: u8) -> u8 {
    if b.is_ascii_uppercase() {
        b + 32
    } else {
        b
    }
}

#[inline]
fn eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(&x, &y)| ascii_lower(x) == ascii_lower(y))
}

#[inline]
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Emulate C `atoi` on a byte slice.
pub fn c_atoi(bytes: &[u8]) -> i32 {
    let mut i = 0;
    while i < bytes.len() && is_c_space(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        return 0;
    }
    let mut neg = false;
    if bytes[i] == b'-' {
        neg = true;
        i += 1;
    } else if bytes[i] == b'+' {
        i += 1;
    }
    let mut acc: i64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        acc = acc.wrapping_mul(10).wrapping_add((bytes[i] - b'0') as i64);
        i += 1;
    }
    if neg {
        (-acc) as i32
    } else {
        acc as i32
    }
}

/// Parse `%i` style 64-bit integer (base 10, `0x` hex, or `0` octal).
fn parse_scanf_i64(bytes: &[u8]) -> Option<(i64, usize)> {
    let mut i = 0;
    while i < bytes.len() && is_c_space(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let mut neg = false;
    if bytes[i] == b'-' {
        neg = true;
        i += 1;
    } else if bytes[i] == b'+' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let (radix, mut digits_start) = if bytes[i] == b'0' {
        if i + 2 <= bytes.len()
            && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X')
            && i + 2 < bytes.len()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            (16u64, i + 2)
        } else {
            (8u64, i)
        }
    } else {
        (10u64, i)
    };
    let start_digits = digits_start;
    let mut acc: u64 = 0;
    let mut overflow = false;
    while digits_start < bytes.len() {
        let d = match bytes[digits_start] {
            b'0'..=b'7' => (bytes[digits_start] - b'0') as u64,
            b'8'..=b'9' if radix >= 10 => (bytes[digits_start] - b'0') as u64,
            b'a'..=b'f' if radix == 16 => (bytes[digits_start] - b'a' + 10) as u64,
            b'A'..=b'F' if radix == 16 => (bytes[digits_start] - b'A' + 10) as u64,
            _ => break,
        };
        if let Some(next) = acc.checked_mul(radix).and_then(|v| v.checked_add(d)) {
            acc = next;
        } else {
            overflow = true;
        }
        digits_start += 1;
    }
    if digits_start == start_digits {
        return None;
    }
    let val: i64 = if neg {
        let max_neg = (i64::MAX as u64) + 1;
        if overflow || acc >= max_neg {
            i64::MIN
        } else {
            -(acc as i64)
        }
    } else if overflow || acc > i64::MAX as u64 {
        i64::MAX
    } else {
        acc as i64
    };
    Some((val, digits_start))
}

/// Parse `%i` style integer (base 10, `0x` hex, or `0` octal).
fn parse_scanf_i32(bytes: &[u8]) -> Option<(i32, usize)> {
    parse_scanf_i64(bytes).map(|(v, used)| (v as i32, used))
}

const FORMATVERSION_SIGNATURE: &[u8] = b"nzbget diskstate file version ";

/// `StateFile::ParseFormatVersion(const char* formatSignature)`
pub fn parse_format_version(format_signature: &[u8]) -> i32 {
    if !format_signature.starts_with(FORMATVERSION_SIGNATURE) {
        return 0;
    }
    c_atoi(&format_signature[FORMATVERSION_SIGNATURE.len()..])
}

/// `StateDiskFile::PrintLine` line-break sanitization: replaces `\n` and `\r` with `' '`.
pub fn sanitize_line_breaks_in_place(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        if *b == b'\n' || *b == b'\r' {
            *b = b' ';
        }
    }
}

/// `DiskState::RaiseIdsPastQueueDir` `"nzb %i file %i"` parser.
pub fn parse_ids_file(text: &[u8]) -> Option<(i32, i32)> {
    if !text.starts_with(b"nzb") {
        return None;
    }
    let rest = &text[3..];
    if rest.is_empty() || !is_c_space(rest[0]) {
        return None;
    }
    let (nzb_id, used1) = parse_scanf_i32(rest)?;
    let mut after1 = &rest[used1..];
    if after1.is_empty() || !is_c_space(after1[0]) {
        return None;
    }
    while !after1.is_empty() && is_c_space(after1[0]) {
        after1 = &after1[1..];
    }
    if !after1.starts_with(b"file") {
        return None;
    }
    let rest2 = &after1[4..];
    if rest2.is_empty() || !is_c_space(rest2[0]) {
        return None;
    }
    let (file_id, _) = parse_scanf_i32(rest2)?;
    if nzb_id >= 0 && file_id >= 0 {
        Some((nzb_id, file_id))
    } else {
        None
    }
}

/// `DiskState::SaveIdFloor` (`daemon/queue/DiskState.cpp`):
/// Atomically writes `"nzb <nzb_id> file <file_id>\n"` to `<queue_dir>/ids.new`
/// and renames it over `<queue_dir>/ids`, cleaning up `<queue_dir>/ids.new` on failure.
pub fn save_id_floor_file(queue_dir: &[u8], sep: u8, nzb_id: i32, file_id: i32) -> bool {
    #[cfg(unix)]
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    let ids_bytes =
        crate::artwritercore::article_writer_format_dest_file_path(queue_dir, sep, b"ids");
    let temp_bytes = diskstate_format_new_path(&ids_bytes);
    let text = diskstate_format_ids_line(nzb_id, file_id);

    #[cfg(unix)]
    let (ids_path, temp_path) = (
        Path::new(std::ffi::OsStr::from_bytes(&ids_bytes)),
        Path::new(std::ffi::OsStr::from_bytes(&temp_bytes)),
    );
    #[cfg(not(unix))]
    let (ids_str, temp_str) = (
        String::from_utf8_lossy(&ids_bytes).into_owned(),
        String::from_utf8_lossy(&temp_bytes).into_owned(),
    );
    #[cfg(not(unix))]
    let (ids_path, temp_path) = (Path::new(&ids_str), Path::new(&temp_str));

    if std::fs::write(temp_path, &text).is_err() {
        let _ = std::fs::remove_file(temp_path);
        return false;
    }

    if std::fs::rename(temp_path, ids_path).is_err() {
        let _ = std::fs::remove_file(temp_path);
        return false;
    }
    true
}

/// `DiskState::RaiseIdsPastQueueDir` filename classifier.
/// Returns `Some((id, is_nzb_log))` when the filename is `"n<id>.log"` or `"<id>"` / `"<id>s"` / `"<id>c"`.
pub fn classify_queue_dir_id(filename: &[u8]) -> Option<(i32, bool)> {
    if filename.is_empty() {
        return None;
    }
    let nzb_log = filename[0] == b'n';
    let p = if nzb_log { &filename[1..] } else { filename };
    if p.is_empty() || !p[0].is_ascii_digit() {
        return None;
    }
    let mut num_end = 0;
    let mut id: i64 = 0;
    while num_end < p.len() && p[num_end].is_ascii_digit() {
        id = id.saturating_mul(10).saturating_add((p[num_end] - b'0') as i64);
        num_end += 1;
    }
    if id <= 0 || id > i32::MAX as i64 {
        return None;
    }
    let suffix = &p[num_end..];
    if nzb_log && suffix == b".log" {
        Some((id as i32, true))
    } else if !nzb_log && (suffix.is_empty() || suffix == b"s" || suffix == b"c") {
        Some((id as i32, false))
    } else {
        None
    }
}

/// `DiskState::ConvertLegacyFileStates` candidate classifier.
/// `suffix_kind`: `0` = `""`, `1` = `"s"`, `2` = `"c"`.
pub fn classify_legacy_file_state(
    filename: &[u8],
    header_line: &[u8],
    file_version: i32,
    legacy_version: i32,
) -> Option<(i32, i32)> {
    let mut num_end = 0;
    while num_end < filename.len() && filename[num_end].is_ascii_digit() {
        num_end += 1;
    }
    if num_end == 0 {
        return None;
    }
    let suffix = &filename[num_end..];
    let suffix_kind = if suffix.is_empty() {
        0
    } else if suffix == b"s" {
        1
    } else if suffix == b"c" {
        2
    } else {
        return None;
    };

    if !header_line.starts_with(FORMATVERSION_SIGNATURE) {
        return None;
    }
    let (version, _) = parse_scanf_i32(&header_line[FORMATVERSION_SIGNATURE.len()..])?;
    if version > file_version && version <= legacy_version {
        Some((c_atoi(filename), suffix_kind))
    } else {
        None
    }
}

#[inline]
pub fn join_i64(high: u32, low: u32) -> i64 {
    (((high as u64) << 32) | (low as u64)) as i64
}

/// `DiskState::LoadDupeExtras` hole offset/size validator.
pub fn validate_repair_hole(
    offset_hi: u32,
    offset_lo: u32,
    size_hi: u32,
    size_lo: u32,
    decoded_size: i64,
) -> Option<(i64, i64)> {
    let offset = join_i64(offset_hi, offset_lo);
    let size = join_i64(size_hi, size_lo);
    if offset < 0 || size <= 0 || decoded_size <= 0 || offset > decoded_size || size > decoded_size - offset {
        None
    } else {
        Some((offset, size))
    }
}

/// `DiskState::SaveDupeTrailer` predicate for whether an `NzbInfo` has dupe extras.
pub fn has_dupe_extras(
    recovered_articles: i32,
    recovered_bytes: i64,
    recovered_holes: i32,
    repair_jobs_count: usize,
) -> bool {
    recovered_articles != 0 || recovered_bytes != 0 || recovered_holes != 0 || repair_jobs_count > 0
}

/// `DiskState::SaveDupeTrailer` `UnpackTried` bitmask packer.
pub fn pack_unpack_tried_flags(unpack_tried: bool, par_repaired: bool) -> i32 {
    (if unpack_tried { 1 } else { 0 }) | (if par_repaired { 2 } else { 0 })
}

fn parse_prefixed_or_plain_int(line: &[u8], prefix: &[u8]) -> Option<i32> {
    if line.starts_with(prefix) {
        parse_scanf_i32(&line[prefix.len()..]).map(|(v, _)| v)
    } else {
        parse_scanf_i32(line).map(|(v, _)| v)
    }
}

/// `DiskState::LoadDupeTrailer` / `LoadNzbInfo` unpack state line parser.
pub fn parse_unpack_state_lines(
    line1: &[u8],
    line2: &[u8],
    line3: &[u8],
    ns_running: i32,
    ns_none: i32,
) -> Option<(i32, bool, bool, i32)> {
    let mut direct_status = parse_prefixed_or_plain_int(line1, b"DirectUnpackStatus=")?;
    if direct_status == ns_running {
        direct_status = ns_none;
    }
    let unpack_tried_flags = parse_prefixed_or_plain_int(line2, b"UnpackTried=")?;
    let unpack_tried = (unpack_tried_flags & 1) != 0;
    let par_repaired = (unpack_tried_flags & 2) != 0;
    let last_unpack_status = parse_prefixed_or_plain_int(line3, b"LastUnpackStatus=")?;
    Some((direct_status, unpack_tried, par_repaired, last_unpack_status))
}

/// `DiskState::LoadNzbInfo` post-stage migration and transient stage clamp.
pub fn normalize_post_stage(mut post_stage: i32, format_version: i32, pt_finished: i32) -> i32 {
    if format_version < 59 && post_stage == 6 {
        post_stage += 1;
    } else if format_version < 59 && post_stage > 6 {
        post_stage += 2;
    }
    if post_stage > pt_finished {
        post_stage = pt_finished;
    }
    post_stage
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsArticleLoadEval {
    pub valid: i32,
    pub status: i32,
    pub segment_offset: i64,
    pub segment_size: i32,
    pub dupe_fallback_round: i32,
    pub need_result_filename: i32,
    pub completed_articles: i32,
}

/// `DiskState::LoadFileState` per-article validation and status normalization.
#[allow(clippy::too_many_arguments)]
pub fn eval_article_load_state(
    format_version: i32,
    completed: bool,
    fields: i32,
    status_in: i32,
    segment_offset_in: i64,
    segment_size_in: i32,
    staged_fallback_in: i32,
    part_number: i32,
    staged_file_exists: bool,
    direct_write: bool,
    force_direct_write: bool,
    has_result_filename: bool,
    completed_articles_in: i32,
    total_size: i32,
) -> RsArticleLoadEval {
    const AI_UNDEFINED: i32 = 0;
    const AI_RUNNING: i32 = 1;
    const AI_FINISHED: i32 = 2;

    let mut status = status_in;
    let mut segment_offset = segment_offset_in;
    let mut segment_size = segment_size_in;
    let mut dupe_fallback_round = 0;

    if format_version >= 2 {
        let mut staged_fallback = staged_fallback_in;
        if (if format_version >= 9 { fields != 5 } else { fields < 4 })
            || (staged_fallback != 0 && staged_fallback != 1)
        {
            return RsArticleLoadEval {
                valid: 0,
                ..Default::default()
            };
        }
        if !completed && (segment_offset < 0 || segment_size < 0 || segment_size > 1024 * 1024 * 1024) {
            status = AI_UNDEFINED;
            staged_fallback = 0;
            segment_offset = 0;
            segment_size = 0;
        }
        if staged_fallback != 0 && part_number > 0 && staged_file_exists {
            status = AI_FINISHED;
        } else {
            staged_fallback = 0;
        }
        dupe_fallback_round = if !completed && status == AI_FINISHED && staged_fallback != 0 {
            1
        } else {
            0
        };
    } else if fields != 1 {
        return RsArticleLoadEval {
            valid: 0,
            ..Default::default()
        };
    }

    if status == AI_RUNNING {
        status = AI_UNDEFINED;
    }

    if !completed
        && format_version < 9
        && status == AI_FINISHED
        && (direct_write || force_direct_write)
        && part_number > 0
        && staged_file_exists
    {
        dupe_fallback_round = 1;
    }

    let need_result_filename = status == AI_FINISHED
        && ((!direct_write && !force_direct_write) || dupe_fallback_round > 0)
        && !has_result_filename;

    if completed_articles_in == total_size - 1 && !completed {
        status = AI_UNDEFINED;
        dupe_fallback_round = 0;
    }

    let completed_articles = if status != AI_UNDEFINED {
        completed_articles_in + 1
    } else {
        completed_articles_in
    };

    RsArticleLoadEval {
        valid: 1,
        status,
        segment_offset,
        segment_size,
        dupe_fallback_round,
        need_result_filename: if need_result_filename { 1 } else { 0 },
        completed_articles,
    }
}

/// `DiskState::LoadFileState` DirectWrite `OutputInitialized` reconciliation.
/// Returns `(new_output_initialized, pick_path_kind)` where `pick_path_kind` is:
/// - `0`: keep current `OutputFilename`
/// - `1`: set `OutputFilename` to `<destDir>/<id>.out.tmp`
/// - `2`: set `OutputFilename` to `<destDir>/<filename>`
pub fn reconcile_direct_write_output(
    output_initialized: bool,
    has_own_out_file: bool,
    has_out_tmp: bool,
    has_completed_dest: bool,
    success_articles: i32,
    completed: bool,
    output_filename_empty: bool,
) -> (bool, i32) {
    let has_direct_write_output = has_own_out_file || has_out_tmp || has_completed_dest;
    let mut new_init = output_initialized;
    if new_init && !has_direct_write_output {
        new_init = false;
    } else if !new_init && has_direct_write_output && (success_articles > 0 || completed) {
        new_init = true;
    }

    let pick_path_kind = if new_init && output_filename_empty && (has_out_tmp || has_completed_dest) {
        if has_out_tmp {
            1
        } else {
            2
        }
    } else {
        0
    };

    (new_init, pick_path_kind)
}

/// `DiskState::CleanupTempDir` filename classifier.
/// Returns:
/// - `(1, 0, 0)` if immediate garbage (`.tmp` or `.dec` substring)
/// - `(2, id, part)` if matches `%i.%i` (caller checks whether `id` is in queue)
/// - `(0, 0, 0)` otherwise
pub fn classify_temp_file(filename: &[u8]) -> (i32, i32, i32) {
    if filename.windows(4).any(|w| w == b".tmp" || w == b".dec") {
        return (1, 0, 0);
    }
    if let Some((id, used1)) = parse_scanf_i32(filename) {
        let rest = &filename[used1..];
        if rest.starts_with(b".") {
            let after_dot = &rest[1..];
            if !after_dot.is_empty() && !is_c_space(after_dot[0]) {
                if let Some((part, _)) = parse_scanf_i32(after_dot) {
                    return (2, id, part);
                }
            }
        }
    }
    (0, 0, 0)
}

/// `DiskState::CleanupQueueDir` orphan file detector.
pub fn is_orphan_queue_file(
    filename: &[u8],
    sorted_file_ids: &[i32],
    sorted_nzb_ids: &[i32],
) -> bool {
    let mut del = false;

    if let Some((id, used)) = parse_scanf_i32(filename) {
        let rest = &filename[used..];
        let is_state_suffix = !rest.is_empty() && (rest[0] == b's' || rest[0] == b'c');
        let is_plain_id = !filename.contains(&b'.');
        if is_state_suffix || is_plain_id {
            del = sorted_file_ids.binary_search(&id).is_err();
        }
    }

    if !del && filename.starts_with(b"n") {
        let after_n = &filename[1..];
        if !after_n.is_empty() && !is_c_space(after_n[0]) {
            if let Some((id, used)) = parse_scanf_i32(after_n) {
                if after_n[used..].starts_with(b".log") {
                    del = sorted_nzb_ids.binary_search(&id).is_err();
                }
            }
        }
    }

    del
}

#[derive(Debug, Clone)]
pub struct ServerMatchInput<'a> {
    pub name: &'a [u8],
    pub host: &'a [u8],
    pub port: i32,
    pub user: &'a [u8],
}

#[derive(Debug, Clone)]
pub struct ServerRefEntry<'a> {
    pub state_id: i32,
    pub name: &'a [u8],
    pub host: &'a [u8],
    pub port: i32,
    pub user: &'a [u8],
    pub matched: bool,
    pub perfect: bool,
}

#[derive(Clone, Copy)]
enum MatchCriteria {
    Name,
    Host,
    Port,
    User,
}

fn find_candidates(
    server: &ServerMatchInput<'_>,
    refs: &[ServerRefEntry<'_>],
    candidates: &mut Vec<usize>,
    criteria: MatchCriteria,
    keep_if_nothing: bool,
) {
    let orig = candidates.clone();
    candidates.retain(|&idx| {
        let r = &refs[idx];
        let m = match criteria {
            MatchCriteria::Name => eq_ignore_ascii_case(server.name, r.name),
            MatchCriteria::Host => eq_ignore_ascii_case(server.host, r.host),
            MatchCriteria::Port => server.port == r.port,
            MatchCriteria::User => eq_ignore_ascii_case(server.user, r.user),
        };
        m && !r.matched
    });
    if candidates.is_empty() && keep_if_nothing {
        *candidates = orig;
    }
}

/// `DiskState::MatchServers` 2-pass server state ID matcher.
/// Populates `out_server_state_ids` (length `servers.len()`) and returns `perfect_match`.
pub fn match_servers(
    servers: &[ServerMatchInput<'_>],
    refs: &mut [ServerRefEntry<'_>],
    out_server_state_ids: &mut [i32],
) -> bool {
    for slot in out_server_state_ids.iter_mut() {
        *slot = 0;
    }

    // Step 1: trying perfect match
    for (s_idx, server) in servers.iter().enumerate() {
        let mut matched_refs: Vec<usize> = (0..refs.len()).collect();
        find_candidates(server, refs, &mut matched_refs, MatchCriteria::Name, false);
        find_candidates(server, refs, &mut matched_refs, MatchCriteria::Host, false);
        find_candidates(server, refs, &mut matched_refs, MatchCriteria::Port, false);
        find_candidates(server, refs, &mut matched_refs, MatchCriteria::User, false);

        if matched_refs.len() == 1 {
            let r_idx = matched_refs[0];
            out_server_state_ids[s_idx] = refs[r_idx].state_id;
            refs[r_idx].matched = true;
            refs[r_idx].perfect = true;
        }
    }

    // Step 2: matching host, port, username and server-name
    for (s_idx, server) in servers.iter().enumerate() {
        if out_server_state_ids[s_idx] == 0 {
            let mut matched_refs: Vec<usize> = (0..refs.len()).collect();
            find_candidates(server, refs, &mut matched_refs, MatchCriteria::Host, false);

            if matched_refs.len() > 1 {
                find_candidates(server, refs, &mut matched_refs, MatchCriteria::Name, true);
            }
            if matched_refs.len() > 1 {
                find_candidates(server, refs, &mut matched_refs, MatchCriteria::User, true);
            }
            if matched_refs.len() > 1 {
                find_candidates(server, refs, &mut matched_refs, MatchCriteria::Port, true);
            }

            if !matched_refs.is_empty() {
                let r_idx = matched_refs[0];
                out_server_state_ids[s_idx] = refs[r_idx].state_id;
                refs[r_idx].matched = true;
            }
        }
    }

    let all_refs_perfect = refs.iter().all(|r| r.perfect);
    let all_servers_matched = out_server_state_ids.iter().all(|&id| id != 0);
    all_refs_perfect && all_servers_matched
}

/// `DiskState::AppendNzbMessage` bad-character sanitizer (`\n`, `\r`, `\t` -> `' '`).
pub fn sanitize_log_text_in_place(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        if *b == b'\n' || *b == b'\r' || *b == b'\t' {
            *b = b' ';
        }
    }
}

/// `DiskState::LoadNzbMessages` log line parser.
/// Returns `Some((time, kind, text_offset))` where `kind` is `Message::EKind`
/// (`mkInfo = 0, mkWarning = 1, mkError = 2, mkDebug = 3, mkDetail = 4`).
pub fn parse_log_line(line: &[u8]) -> Option<(i32, i32, usize)> {
    let tab1 = line.iter().position(|&b| b == b'\t')?;
    let after1 = &line[tab1 + 1..];
    let time = c_atoi(after1);

    let tab2_rel = after1.iter().position(|&b| b == b'\t')?;
    let kind_slice = &after1[tab2_rel + 1..];
    let kind = if kind_slice.starts_with(b"INFO") {
        0 // mkInfo
    } else if kind_slice.starts_with(b"WARNING") {
        1 // mkWarning
    } else if kind_slice.starts_with(b"ERROR") {
        2 // mkError
    } else if kind_slice.starts_with(b"DETAIL") {
        4 // mkDetail
    } else if kind_slice.starts_with(b"DEBUG") {
        3 // mkDebug
    } else {
        2 // mkError
    };

    let tab3_rel = kind_slice.iter().position(|&b| b == b'\t')?;
    let text_offset = (tab1 + 1) + (tab2_rel + 1) + (tab3_rel + 1);
    Some((time, kind, text_offset))
}

/// Validates the 4-field header of `DiskState::LoadDupeExtras`.
#[inline]
pub fn validate_dupe_extras_header(
    scan_cnt: i32,
    recovered_articles: i32,
    recovered_holes: i32,
) -> bool {
    scan_cnt == 4 && recovered_articles >= 0 && recovered_holes >= 0
}

/// Validates a per-repair-job header line in `DiskState::LoadDupeExtras`.
#[inline]
pub fn validate_repair_job_header(
    scan_cnt: i32,
    expected_cnt: i32,
    file_id: i32,
    par_file: i32,
    failed_articles: i32,
) -> bool {
    scan_cnt == expected_cnt && failed_articles >= 0 && file_id > 0 && (0..=1).contains(&par_file)
}

/// Evaluates whether a restored `OutputInitialized` flag remains valid in `DiskState::LoadFileInfo`.
#[inline]
pub fn eval_restored_output_init(
    output_initialized: i32,
    direct_write: bool,
    force_direct_write: bool,
    has_own_out_file: bool,
    has_out_tmp: bool,
    has_force_dest: bool,
) -> bool {
    let restored = output_initialized != 0;
    if restored && (direct_write || force_direct_write) {
        has_own_out_file || has_out_tmp || has_force_dest
    } else {
        restored
    }
}

/// Computes `(staged_fallback, saved_status)` when serializing an `ArticleInfo` in `DiskState::SaveFileState`.
#[inline]
pub fn calc_staged_fallback(
    completed: bool,
    article_status: i32,
    ai_finished: i32,
    dupe_fallback_round: i32,
    has_result_filename: bool,
    ai_undefined: i32,
) -> (i32, i32) {
    let staged = if !completed
        && article_status == ai_finished
        && dupe_fallback_round > 0
        && has_result_filename
    {
        1
    } else {
        0
    };
    let saved_status = if staged != 0 {
        ai_undefined
    } else {
        article_status
    };
    (staged, saved_status)
}

/// Validates the article count header in `DiskState::LoadFileState`.
#[inline]
pub fn validate_article_count(size: i32, has_articles: bool, articles_len: i32) -> bool {
    !(size < 0 || (has_articles && size > articles_len) || size > 10_000_000)
}

/// Determines whether `DiskState::LoadFileState` needs to probe the temp directory for a staged part file.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn need_staged_file_check(
    completed: bool,
    format_version: i32,
    status_int: i32,
    ai_finished: i32,
    segment_offset: i64,
    segment_size: i32,
    staged_fallback: i32,
    part_number: i32,
    direct_write: bool,
    force_direct_write: bool,
) -> bool {
    let range_valid =
        completed || (segment_offset >= 0 && (0..=1024 * 1024 * 1024).contains(&segment_size));
    let effective_finished = status_int == ai_finished && (format_version < 2 || range_valid);
    part_number > 0
        && ((format_version >= 2 && staged_fallback == 1 && range_valid)
            || (!completed
                && format_version < 9
                && effective_finished
                && (direct_write || force_direct_write)))
}

/// Validates and computes the target array length for a server volume slot in `DiskState::LoadVolumeStat`.
#[inline]
pub fn calc_volume_slot_size(slot_index: i32, arr_size: i32) -> (bool, i32) {
    if !(0..=100_000).contains(&arr_size) {
        return (false, 0);
    }
    let fixed = match slot_index {
        0 | 1 => 60,
        2 => 24,
        _ => 0,
    };
    (true, if fixed > 0 { fixed } else { arr_size })
}

/// `StateDiskFile::ReadLine` (`DiskState.cpp`):
/// Given a NUL-terminated line buffer of length `len`, returns `(needs_scroll, new_len)`.
pub fn inspect_read_line(buf: &[u8]) -> (bool, usize) {
    if buf.is_empty() {
        (false, 0)
    } else {
        let last = buf[buf.len() - 1];
        (last != b'\n', buf.len() - 1)
    }
}

/// `DiskState::LoadDownloadQueue` (`DiskState.cpp`):
/// Evaluates `format_version` against `queue_version` (64).
pub fn check_queue_format_version(
    format_version: i32,
    queue_version: i32,
) -> (i32, bool, bool) {
    let is_legacy = format_version != queue_version;
    if format_version <= 0 {
        (1, is_legacy, false)
    } else if format_version < 47 {
        (2, is_legacy, false)
    } else {
        (0, is_legacy, format_version < 57)
    }
}

/// Parse `%u` style integer (base 10).
fn parse_scanf_u32(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let mut i = 0;
    while i < bytes.len() && is_c_space(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let mut neg = false;
    if bytes[i] == b'-' {
        neg = true;
        i += 1;
    } else if bytes[i] == b'+' {
        i += 1;
    }
    let start_digits = i;
    let mut acc: u64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        let d = (bytes[i] - b'0') as u64;
        acc = acc.saturating_mul(10).saturating_add(d);
        i += 1;
    }
    if i == start_digits {
        return None;
    }
    let val = if neg { acc.wrapping_neg() } else { acc };
    Some((val as u32, &bytes[i..]))
}

/// `DiskState::LoadFileState` (`DiskState.cpp`):
/// Parses a `< 8` format trailer line (`#decoded <u32>,<u32>` or `OutputInitialized=<i32>`).
pub fn parse_legacy_file_state_trailer(line: &[u8]) -> (i32, i64, i32) {
    if let Some(rest) = line.strip_prefix(b"#decoded ") {
        if let Some((hi, after_hi)) = parse_scanf_u32(rest) {
            if let Some(after_comma) = after_hi.strip_prefix(b",") {
                if let Some((lo, _)) = parse_scanf_u32(after_comma) {
                    return (1, join_i64(hi, lo), 0);
                }
            }
        }
    }
    if let Some(rest) = line.strip_prefix(b"OutputInitialized=") {
        if let Some((val, _)) = parse_scanf_i32(rest) {
            return (2, 0, if val != 0 { 1 } else { 0 });
        }
    }
    (0, 0, 0)
}

/// `DiskState::LoadFeedStatus` (`DiskState.cpp`):
/// Returns `1` if `feed_url == url` and the format-version-specific filter check matches.
pub fn feed_status_matches(
    feed_url: &[u8],
    url: &[u8],
    format_version: i32,
    feed_filter: &[u8],
    filter: &[u8],
    feed_filter_hash: u32,
    filter_hash: u32,
) -> i32 {
    if feed_url != url {
        return 0;
    }
    let ok = format_version == 1
        || (format_version == 2 && feed_filter == filter)
        || (format_version >= 3 && feed_filter_hash == filter_hash);
    i32::from(ok)
}

pub type ReadLineFn =
    unsafe extern "C" fn(ctx: *mut std::ffi::c_void, max_size: c_int) -> *const c_char;

struct ScanCursor<'a>(&'a [u8]);

impl<'a> ScanCursor<'a> {
    fn i32(&mut self, comma: bool) -> Option<i32> {
        if comma {
            self.0 = self.0.strip_prefix(b",")?;
        }
        let (v, used) = parse_scanf_i32(self.0)?;
        self.0 = &self.0[used..];
        Some(v)
    }

    fn i64(&mut self, comma: bool) -> Option<i64> {
        if comma {
            self.0 = self.0.strip_prefix(b",")?;
        }
        let (v, used) = parse_scanf_i64(self.0)?;
        self.0 = &self.0[used..];
        Some(v)
    }

    fn u32(&mut self, comma: bool) -> Option<u32> {
        if comma {
            self.0 = self.0.strip_prefix(b",")?;
        }
        let (v, rest) = parse_scanf_u32(self.0)?;
        self.0 = rest;
        Some(v)
    }
}

unsafe fn read_line_bytes(
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
    max_size: c_int,
) -> Option<Vec<u8>> {
    let ptr = unsafe { read_line_cb(infile_ctx, max_size) };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { std::ffi::CStr::from_ptr(ptr).to_bytes().to_vec() })
    }
}

unsafe fn read_line_cstr(
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
    max_size: c_int,
) -> Option<Vec<u8>> {
    let mut v = unsafe { read_line_bytes(infile_ctx, read_line_cb, max_size) }?;
    v.push(0);
    Some(v)
}

unsafe fn scan_one_i32(
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
) -> Option<i32> {
    let line = unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }?;
    ScanCursor(&line).i32(false)
}

/// Consolidated filesystem operations for `DiskState`:
/// - `op == 0`: `DiscardDownloadQueue`
/// - `op == 1`: `DownloadQueueExists`
/// - `op == 2`: `DiscardFile(a1, a2 != 0, a3 != 0, a4 != 0)`
/// - `op == 3`: `DeleteCacheFlag`
/// - `op == 4`: `RaiseIdsPastQueueDir` (outputs `max_nzb_id` and `max_file_id` into `out1`/`out2`)
/// - `op == 5`: `SaveIdFloor(a1, a2)`
/// - `op == 6`: `CacheWasActive` (checks `<queue_dir><sep>acache`)
/// - `op == 7`: `DeleteQueueNzbLog(a1)` (deletes `<queue_dir><sep>n<a1>.log`)
#[allow(clippy::too_many_arguments)]
pub fn diskstate_fs_op(
    op: i32,
    qdir: &[u8],
    sep: u8,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    out1: &mut i32,
    out2: &mut i32,
) -> i32 {
    let join = |name: &[u8]| -> Vec<u8> {
        crate::artwritercore::article_writer_format_dest_file_path(qdir, sep, name)
    };
    let remove = |name: &[u8]| {
        let p = join(name);
        let _ = std::fs::remove_file(crate::fsops::path(&p));
    };
    let exists = |name: &[u8]| -> bool {
        let p = join(name);
        crate::fsops::posix_stat(&p) == 1
    };

    match op {
        0 => {
            remove(b"queue");
            remove(b"history");
            for entry in crate::fsops::list_dir_entries(qdir) {
                if !entry.is_empty() && entry.iter().all(|b| b.is_ascii_digit()) {
                    remove(&entry);
                    let mut s_name = entry.clone();
                    s_name.push(b's');
                    remove(&s_name);
                    let mut c_name = entry;
                    c_name.push(b'c');
                    remove(&c_name);
                }
            }
            1
        }
        1 => i32::from(exists(b"queue") || exists(b"history") || exists(b"ids")),
        2 => {
            if a2 != 0 {
                remove(format!("{a1}").as_bytes());
            }
            if a3 != 0 {
                remove(format!("{a1}s").as_bytes());
            }
            if a4 != 0 {
                remove(format!("{a1}c").as_bytes());
            }
            1
        }
        3 => {
            remove(b"acache");
            1
        }
        4 => {
            let mut max_nzb = -1i32;
            let mut max_file = -1i32;
            let ids_path = join(b"ids");
            if crate::fsops::posix_stat(&ids_path) == 1 {
                if let Ok(mut data) = std::fs::read(crate::fsops::path(&ids_path)) {
                    data.push(0);
                    if let Some((n_id, f_id)) = parse_ids_file(&data) {
                        max_nzb = max_nzb.max(n_id);
                        max_file = max_file.max(f_id);
                    }
                }
            }
            for entry in crate::fsops::list_dir_entries(qdir) {
                if let Some((id, is_nzb_log)) = classify_queue_dir_id(&entry) {
                    if is_nzb_log {
                        max_nzb = max_nzb.max(id);
                    } else {
                        max_file = max_file.max(id);
                    }
                }
            }
            *out1 = max_nzb;
            *out2 = max_file;
            1
        }
        5 => i32::from(save_id_floor_file(qdir, sep, a1, a2)),
        6 => i32::from(exists(b"acache")),
        7 => {
            remove(format!("n{a1}.log").as_bytes());
            1
        }
        _ => 0,
    }
}

/// Reads saved server info from `infile_ctx` via `read_line_cb` and runs `match_servers`.
/// Returns `Some(perfect_match)` on success or `None` on read/parse error.
pub unsafe fn load_server_refs_and_match(
    servers: &[ServerMatchInput<'_>],
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
    out_state_ids: &mut [i32],
) -> Option<bool> {
    let size = unsafe { scan_one_i32(infile_ctx, read_line_cb) }?;
    if !(0..=100_000).contains(&size) {
        return None;
    }
    let count = size as usize;
    let mut names = Vec::with_capacity(count);
    let mut hosts = Vec::with_capacity(count);
    let mut ports = Vec::with_capacity(count);
    let mut users = Vec::with_capacity(count);
    for _ in 0..count {
        let name = unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }?;
        let host = unsafe { read_line_bytes(infile_ctx, read_line_cb, 200) }?;
        let port = unsafe { scan_one_i32(infile_ctx, read_line_cb) }?;
        let user = unsafe { read_line_bytes(infile_ctx, read_line_cb, 100) }?;
        names.push(name);
        hosts.push(host);
        ports.push(port);
        users.push(user);
    }
    let mut refs = Vec::with_capacity(count);
    for idx in 0..count {
        refs.push(ServerRefEntry {
            state_id: (idx + 1) as i32,
            name: &names[idx],
            host: &hosts[idx],
            port: ports[idx],
            user: &users[idx],
            matched: false,
            perfect: false,
        });
    }
    Some(match_servers(servers, &mut refs, out_state_ids))
}

/// Parses `DiskState::LoadDupeExtras` via `read_line_cb`.
pub unsafe fn load_dupe_extras(
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
    format_version: c_int,
    target_ctx: *mut std::ffi::c_void,
    on_header_cb: unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        rec_articles: c_int,
        rec_bytes: i64,
        rec_holes: c_int,
    ),
    on_job_cb: unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        job: *const NzbgetRsDiskRepairJob,
    ),
) -> bool {
    let Some(hdr_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut cur = ScanCursor(&hdr_line);
    let (Some(rec_articles), Some(rec_hi), Some(rec_lo), Some(rec_holes)) =
        (cur.i32(false), cur.u32(true), cur.u32(true), cur.i32(true))
    else {
        return false;
    };
    if !validate_dupe_extras_header(4, rec_articles, rec_holes) {
        return false;
    }
    unsafe {
        on_header_cb(
            target_ctx,
            rec_articles,
            join_i64(rec_hi, rec_lo),
            rec_holes,
        );
    }

    let Some(repair_count) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };
    if !(0..=100_000).contains(&repair_count) {
        return false;
    }
    let want_steps = format_version >= 66;
    let expected_fields = if want_steps { 11 } else { 9 };
    for _ in 0..repair_count {
        let Some(job_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        let mut jc = ScanCursor(&job_line);
        let (
            Some(file_id),
            Some(dec_hi),
            Some(dec_lo),
            Some(fail_hi),
            Some(fail_lo),
            Some(miss_hi),
            Some(miss_lo),
            Some(par_file),
            Some(failed_articles),
        ) = (
            jc.i32(false),
            jc.u32(true),
            jc.u32(true),
            jc.u32(true),
            jc.u32(true),
            jc.u32(true),
            jc.u32(true),
            jc.i32(true),
            jc.i32(true),
        )
        else {
            return false;
        };
        let (steps_hi, steps_lo) = if want_steps {
            let (Some(sh), Some(sl)) = (jc.u32(true), jc.u32(true)) else {
                return false;
            };
            (sh, sl)
        } else {
            (0, 0)
        };
        if !validate_repair_job_header(
            expected_fields,
            expected_fields,
            file_id,
            par_file,
            failed_articles,
        ) {
            return false;
        }
        let Some(filename_cstr) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) })
        else {
            return false;
        };
        let Some(hole_count) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
            return false;
        };
        if !(0..=1_000_000).contains(&hole_count) {
            return false;
        }
        let decoded_size = join_i64(dec_hi, dec_lo);
        let mut holes = Vec::with_capacity(hole_count as usize);
        for _ in 0..hole_count {
            let Some(hole_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) })
            else {
                return false;
            };
            let mut hc = ScanCursor(&hole_line);
            let (Some(oh), Some(ol), Some(sh), Some(sl)) =
                (hc.u32(false), hc.u32(true), hc.u32(true), hc.u32(true))
            else {
                return false;
            };
            let Some((offset, size)) = validate_repair_hole(oh, ol, sh, sl, decoded_size) else {
                return false;
            };
            holes.push(NzbgetRsDiskStreamHole { offset, size });
        }
        let job = NzbgetRsDiskRepairJob {
            file_id,
            decoded_file_size: decoded_size,
            failed_size: join_i64(fail_hi, fail_lo),
            missed_size: join_i64(miss_hi, miss_lo),
            par_file,
            failed_articles,
            steps_hash: join_i64(steps_hi, steps_lo) as u64,
            filename: filename_cstr.as_ptr().cast::<c_char>(),
            holes: if holes.is_empty() {
                std::ptr::null()
            } else {
                holes.as_ptr()
            },
            holes_len: holes.len(),
        };
        unsafe { on_job_cb(target_ctx, &job) };
    }
    true
}

/// Parses `DiskState::LoadVolumeStat` via `read_line_cb`.
pub unsafe fn load_volume_stat(
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
    format_version: c_int,
    target_ctx: *mut std::ffi::c_void,
    on_item_cb: unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        slot_idx: c_int,
        item: *const NzbgetRsDiskVolumeStatInput,
        has_custom_times: c_int,
    ),
) -> bool {
    let Some(size) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };
    for i in 0..size {
        let Some(t_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        let Some(b_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        let mut tc = ScanCursor(&t_line);
        let mut bc = ScanCursor(&b_line);
        let (first_day, data_time, custom_time, counters_reset_time, hi1, lo1, hi2, lo2, has_custom) =
            if format_version >= 4 {
                let (Some(fd), Some(dt), Some(ct), Some(crt)) =
                    (tc.i32(false), tc.i32(true), tc.i32(true), tc.i32(true))
                else {
                    return false;
                };
                let (Some(h1), Some(l1), Some(h2), Some(l2)) =
                    (bc.u32(false), bc.u32(true), bc.u32(true), bc.u32(true))
                else {
                    return false;
                };
                (fd, dt, ct, crt, h1, l1, h2, l2, 1)
            } else if format_version == 3 {
                let (Some(fd), Some(dt), Some(ct)) = (tc.i32(false), tc.i32(true), tc.i32(true))
                else {
                    return false;
                };
                let (Some(h1), Some(l1), Some(h2), Some(l2)) =
                    (bc.u32(false), bc.u32(true), bc.u32(true), bc.u32(true))
                else {
                    return false;
                };
                (fd, dt, ct, fd.wrapping_mul(86400), h1, l1, h2, l2, 1)
            } else {
                let (Some(fd), Some(dt)) = (tc.i32(false), tc.i32(true)) else {
                    return false;
                };
                let (Some(h1), Some(l1)) = (bc.u32(false), bc.u32(true)) else {
                    return false;
                };
                (fd, dt, 0, 0, h1, l1, 0, 0, 0)
            };

        let mut slots: [Vec<i64>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for (k, slot) in slots.iter_mut().enumerate() {
            let Some(arr_size) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
                return false;
            };
            let (valid, target_len) = calc_volume_slot_size(k as i32, arr_size);
            if !valid {
                return false;
            }
            slot.resize(target_len as usize, 0);
            for j in 0..arr_size {
                let Some(v_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) })
                else {
                    return false;
                };
                let mut vc = ScanCursor(&v_line);
                let (Some(vh), Some(vl)) = (vc.u32(false), vc.u32(true)) else {
                    return false;
                };
                if (j as usize) < slot.len() {
                    slot[j as usize] = join_i64(vh, vl);
                }
            }
        }

        let mut articles = Vec::new();
        if format_version >= 4 {
            let Some(a_hdr) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
                return false;
            };
            let Some(arr_size) = ScanCursor(&a_hdr).u32(false) else {
                return false;
            };
            if arr_size > 100_000 {
                return false;
            }
            articles.reserve(arr_size as usize);
            for _ in 0..arr_size {
                let Some(a_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) })
                else {
                    return false;
                };
                let mut ac = ScanCursor(&a_line);
                let (Some(failed), Some(success)) = (ac.u32(false), ac.u32(true)) else {
                    return false;
                };
                articles.push(crate::xmlrpc_format::NzbgetRsRpcArticlePair { failed, success });
            }
        }

        let item = NzbgetRsDiskVolumeStatInput {
            first_day,
            data_time,
            custom_time,
            counters_reset_time,
            total_bytes: join_i64(hi1, lo1),
            custom_bytes: join_i64(hi2, lo2),
            bytes_per_seconds: slots[0].as_ptr(),
            bytes_per_seconds_len: slots[0].len(),
            bytes_per_minutes: slots[1].as_ptr(),
            bytes_per_minutes_len: slots[1].len(),
            bytes_per_hours: slots[2].as_ptr(),
            bytes_per_hours_len: slots[2].len(),
            bytes_per_days: slots[3].as_ptr(),
            bytes_per_days_len: slots[3].len(),
            articles_per_days: if articles.is_empty() {
                std::ptr::null()
            } else {
                articles.as_ptr()
            },
            articles_per_days_len: articles.len(),
        };
        unsafe { on_item_cb(target_ctx, i, &item, has_custom) };
    }
    true
}

/// Parses `DiskState::LoadFileInfo` via `read_line_cb`.
#[allow(clippy::too_many_arguments)]
pub unsafe fn load_file_info(
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
    format_version: c_int,
    file_summary: c_int,
    articles: c_int,
    file_id: c_int,
    direct_write_opt: c_int,
    force_direct_write: c_int,
    sep: u8,
    dest_dir: &[u8],
    cur_filename: &[u8],
    cur_output_filename: &[u8],
    target_ctx: *mut std::ffi::c_void,
    on_summary_cb: unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        fields: *const NzbgetRsDiskFileInfoInput,
        flags: c_int,
    ),
    on_item_cb: unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        group_or_null: *const c_char,
        art_or_null: *const NzbgetRsDiskArticleInfo,
    ),
) -> bool {
    let Some(subject_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let Some(filename_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let origname_c = if format_version >= 6 {
        let Some(v) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        v
    } else {
        vec![0]
    };
    let output_filename_c = if format_version >= 7 {
        let Some(v) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        v
    } else {
        vec![0]
    };

    let mut flags: c_int = 0;
    if format_version >= 7 && output_filename_c.len() > 1 {
        flags |= 1;
    }

    let mut filename_confirmed: c_int = 0;
    let mut time_val: c_int = 0;
    let mut output_initialized: c_int = 0;

    if format_version >= 5 {
        let Some(line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        let mut sc = ScanCursor(&line);
        let (Some(fc), Some(tm)) = (sc.i32(false), sc.i32(true)) else {
            return false;
        };
        filename_confirmed = fc;
        time_val = tm;
        if file_summary != 0 {
            flags |= 2 | 4;
        }
        if let Some(oi) = sc.i32(true) {
            if file_summary != 0 {
                flags |= 8;
                let dw = direct_write_opt != 0;
                let fdw = force_direct_write != 0;
                let mut has_own = false;
                let mut has_tmp = false;
                let mut has_force_dest = false;
                if oi != 0 && (dw || fdw) {
                    let eff_out = if (flags & 1) != 0 {
                        &output_filename_c[..output_filename_c.len() - 1]
                    } else {
                        cur_output_filename
                    };
                    if !eff_out.is_empty() {
                        has_own = crate::fsops::posix_stat(eff_out) == 1;
                    }
                    if !dest_dir.is_empty() {
                        let tmp_p = crate::artwritercore::article_writer_format_dest_file_path(
                            dest_dir,
                            sep,
                            format!("{file_id}.out.tmp").as_bytes(),
                        );
                        has_tmp = crate::fsops::posix_stat(&tmp_p) == 1;
                    }
                    let eff_fname = if filename_c.len() > 1 {
                        &filename_c[..filename_c.len() - 1]
                    } else {
                        cur_filename
                    };
                    if fdw && !dest_dir.is_empty() && !eff_fname.is_empty() {
                        let dst_p = crate::artwritercore::article_writer_format_dest_file_path(
                            dest_dir, sep, eff_fname,
                        );
                        has_force_dest = crate::fsops::posix_stat(&dst_p) == 1;
                    }
                }
                output_initialized = c_int::from(eval_restored_output_init(
                    oi,
                    dw,
                    fdw,
                    has_own,
                    has_tmp,
                    has_force_dest,
                ));
            }
        }
    } else if format_version >= 4 {
        let Some(tm) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
            return false;
        };
        time_val = tm;
        if file_summary != 0 {
            flags |= 4;
        }
    }

    let Some(sz_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut szc = ScanCursor(&sz_line);
    let (Some(sz_hi), Some(sz_lo)) = (szc.u32(false), szc.u32(true)) else {
        return false;
    };

    let Some(ms_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut msc = ScanCursor(&ms_line);
    let (Some(ms_hi), Some(ms_lo)) = (msc.u32(false), msc.u32(true)) else {
        return false;
    };

    let Some(par_file) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };

    let Some(art_cnt_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut acc = ScanCursor(&art_cnt_line);
    let (Some(total_articles), Some(missed_articles)) = (acc.i32(false), acc.i32(true)) else {
        return false;
    };

    let fields = NzbgetRsDiskFileInfoInput {
        subject: subject_c.as_ptr().cast::<c_char>(),
        filename: filename_c.as_ptr().cast::<c_char>(),
        origname: origname_c.as_ptr().cast::<c_char>(),
        output_filename: output_filename_c.as_ptr().cast::<c_char>(),
        filename_confirmed,
        time: time_val,
        output_initialized,
        size: join_i64(sz_hi, sz_lo),
        missed_size: join_i64(ms_hi, ms_lo),
        par_file,
        total_articles,
        missed_articles,
    };
    unsafe { on_summary_cb(target_ctx, &fields, flags) };

    let Some(grp_count) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };
    for _ in 0..grp_count {
        let Some(grp_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        if file_summary != 0 && grp_c.len() > 1 {
            unsafe { on_item_cb(target_ctx, grp_c.as_ptr().cast::<c_char>(), std::ptr::null()) };
        }
    }

    if articles != 0 {
        let Some(art_count) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
            return false;
        };
        for _ in 0..art_count {
            let Some(p_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
                return false;
            };
            let mut pc = ScanCursor(&p_line);
            let (Some(part_number), Some(size)) = (pc.i32(false), pc.i32(true)) else {
                return false;
            };
            let Some(msg_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
                return false;
            };
            let art = NzbgetRsDiskArticleInfo {
                part_number,
                size,
                message_id: msg_c.as_ptr().cast::<c_char>(),
            };
            unsafe { on_item_cb(target_ctx, std::ptr::null(), &art) };
        }
    }

    true
}

/// Consolidated section parser / directory scanner for `DiskState`:
/// - `kind == 0`: `LoadDupInfo`
/// - `kind == 1`: `LoadFeedStatus`
/// - `kind == 2`: `LoadFeedHistory`
/// - `kind == 3`: `LoadServerStats`
/// - `kind == 4`: `ConvertLegacyFileStates` directory scan (`format_version` = `DISKSTATE_FILE_VERSION`, `now as i32` = `DISKSTATE_FILE_LEGACY_VERSION`)
/// - `kind == 5`: `SetAsideQueueFiles` (`now` = `Util::CurrentTime()`)
#[allow(clippy::too_many_arguments)]
pub unsafe fn load_section(
    kind: c_int,
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: Option<ReadLineFn>,
    format_version: c_int,
    qdir: &[u8],
    sep: u8,
    now: i64,
    target_ctx: *mut std::ffi::c_void,
    on_entry_cb: unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        a1: c_int,
        a2: c_int,
        a3: c_int,
        u1: u32,
        u2: u32,
        i64_1: i64,
        s1: *const c_char,
        s2: *const c_char,
    ),
) -> bool {
    match kind {
        0 => {
            let Some(cb) = read_line_cb else { return false };
            let Some(hdr) = (unsafe { read_line_bytes(infile_ctx, cb, 1024) }) else {
                return false;
            };
            let mut sc = ScanCursor(&hdr);
            let (
                Some(status),
                Some(hi),
                Some(lo),
                Some(full_hash),
                Some(filt_hash),
                Some(dupe_score),
                Some(dupe_mode),
            ) = (
                sc.i32(false),
                sc.u32(true),
                sc.u32(true),
                sc.u32(true),
                sc.u32(true),
                sc.i32(true),
                sc.i32(true),
            )
            else {
                return false;
            };
            let Some(name_c) = (unsafe { read_line_cstr(infile_ctx, cb, 1024) }) else {
                return false;
            };
            let Some(key_c) = (unsafe { read_line_cstr(infile_ctx, cb, 1024) }) else {
                return false;
            };
            unsafe {
                on_entry_cb(
                    target_ctx,
                    status,
                    dupe_score,
                    dupe_mode,
                    full_hash,
                    filt_hash,
                    join_i64(hi, lo),
                    name_c.as_ptr().cast::<c_char>(),
                    key_c.as_ptr().cast::<c_char>(),
                );
            }
            true
        }
        1 => {
            let Some(cb) = read_line_cb else { return false };
            let Some(size) = (unsafe { scan_one_i32(infile_ctx, cb) }) else {
                return false;
            };
            for _ in 0..size {
                let Some(url_c) = (unsafe { read_line_cstr(infile_ctx, cb, 1024) }) else {
                    return false;
                };
                let filter_c = if format_version == 2 {
                    let Some(f) = (unsafe { read_line_cstr(infile_ctx, cb, 1024) }) else {
                        return false;
                    };
                    f
                } else {
                    vec![0]
                };
                let filter_hash = if format_version >= 3 {
                    let Some(h_line) = (unsafe { read_line_bytes(infile_ctx, cb, 1024) }) else {
                        return false;
                    };
                    let Some(h) = ScanCursor(&h_line).u32(false) else {
                        return false;
                    };
                    h
                } else {
                    0
                };
                let Some(last_update) = (unsafe { scan_one_i32(infile_ctx, cb) }) else {
                    return false;
                };
                unsafe {
                    on_entry_cb(
                        target_ctx,
                        last_update,
                        format_version,
                        0,
                        filter_hash,
                        0,
                        0,
                        url_c.as_ptr().cast::<c_char>(),
                        filter_c.as_ptr().cast::<c_char>(),
                    );
                }
            }
            true
        }
        2 => {
            let Some(cb) = read_line_cb else { return false };
            let Some(size) = (unsafe { scan_one_i32(infile_ctx, cb) }) else {
                return false;
            };
            for _ in 0..size {
                let Some(line) = (unsafe { read_line_bytes(infile_ctx, cb, 1024) }) else {
                    return false;
                };
                let mut sc = ScanCursor(&line);
                let (Some(status), Some(last_seen)) = (sc.i32(false), sc.i32(true)) else {
                    return false;
                };
                let Some(url_c) = (unsafe { read_line_cstr(infile_ctx, cb, 1024) }) else {
                    return false;
                };
                unsafe {
                    on_entry_cb(
                        target_ctx,
                        status,
                        last_seen,
                        0,
                        0,
                        0,
                        0,
                        url_c.as_ptr().cast::<c_char>(),
                        std::ptr::null(),
                    );
                }
            }
            true
        }
        4 => {
            use std::io::Read;
            let legacy_ver = now as i32;
            for entry in crate::fsops::list_dir_entries(qdir) {
                let digits = entry.iter().take_while(|b| b.is_ascii_digit()).count();
                if digits == 0 {
                    continue;
                }
                let rem = &entry[digits..];
                if !(rem.is_empty() || rem == b"s" || rem == b"c") {
                    continue;
                }
                let full_p = crate::artwritercore::article_writer_format_dest_file_path(
                    qdir, sep, &entry,
                );
                let Ok(mut f) = std::fs::File::open(crate::fsops::path(&full_p)) else {
                    continue;
                };
                let mut buf = [0u8; 99];
                let Ok(n) = f.read(&mut buf) else {
                    continue;
                };
                if n == 0 {
                    continue;
                }
                let line_end = buf[..n].iter().position(|&b| b == b'\n').map_or(n, |p| p + 1);
                if let Some((id, suffix_kind)) = classify_legacy_file_state(
                    &entry,
                    &buf[..line_end],
                    format_version,
                    legacy_ver,
                ) {
                    unsafe {
                        on_entry_cb(
                            target_ctx,
                            id,
                            suffix_kind,
                            0,
                            0,
                            0,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                        );
                    }
                }
            }
            true
        }
        5 => {
            for name in ["queue", "history", "progress"] {
                let src = crate::artwritercore::article_writer_format_dest_file_path(
                    qdir,
                    sep,
                    name.as_bytes(),
                );
                if crate::fsops::posix_stat(&src) == 1 {
                    let kept_base = format!("{name}.unreadable-{now}");
                    let dst = crate::artwritercore::article_writer_format_dest_file_path(
                        qdir,
                        sep,
                        kept_base.as_bytes(),
                    );
                    if let (Ok(src_c), Ok(dst_c), Ok(name_c), Ok(base_c)) = (
                        std::ffi::CString::new(src),
                        std::ffi::CString::new(dst),
                        std::ffi::CString::new(name),
                        std::ffi::CString::new(kept_base),
                    ) {
                        if crate::fsops::move_file(&src_c, &dst_c) {
                            unsafe {
                                on_entry_cb(
                                    target_ctx,
                                    0,
                                    0,
                                    0,
                                    0,
                                    0,
                                    0,
                                    name_c.as_ptr(),
                                    base_c.as_ptr(),
                                );
                            }
                        }
                    }
                }
            }
            true
        }
        _ => false,
    }
}

unsafe fn read_server_stats(
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
) -> Option<Vec<NzbgetRsDiskServerStat>> {
    let stat_count = unsafe { scan_one_i32(infile_ctx, read_line_cb) }?;
    let mut stats = Vec::with_capacity(stat_count.clamp(0, 256) as usize);
    for _ in 0..stat_count {
        let line = unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }?;
        let mut sc = ScanCursor(&line);
        let (server_id, success_articles, failed_articles) =
            (sc.i32(false)?, sc.i32(true)?, sc.i32(true)?);
        stats.push(NzbgetRsDiskServerStat {
            server_id,
            success_articles,
            failed_articles,
        });
    }
    Some(stats)
}

/// Parses `DiskState::LoadNzbInfo` via `read_line_cb`.
/// `flags` bitmask passed to `on_loaded_cb`:
/// - `1`: `format_version >= 51` (`par_full`, `force_par_full`, `force_repair`, `extra_par_blocks`)
/// - `2`: `format_version >= 48` (`downloaded_size`, `download_sec`, `post_total_sec`, `par_sec`, `repair_sec`, `unpack_sec`)
/// - `4`: 4-field parameter header (`skip_script_processing`, `skip_disk_write`, `desired_server_id`)
/// - `8`: `format_version < 56` (`file_times` populated)
pub unsafe fn load_nzb_info(
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
    format_version: c_int,
    pt_finished: c_int,
    target_ctx: *mut std::ffi::c_void,
    on_loaded_cb: unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        inp: *const NzbgetRsDiskNzbInfoInput,
        flags: c_int,
        completed: *const NzbgetRsDiskCompletedFile,
        completed_len: usize,
        params: *const NzbgetRsDiskParam,
        params_len: usize,
        scripts: *const NzbgetRsDiskScriptStatus,
        scripts_len: usize,
        stats: *const NzbgetRsDiskServerStat,
        stats_len: usize,
        files: *const NzbgetRsDiskFileStub,
        file_times: *const c_int,
        files_len: usize,
    ),
) -> bool {
    let Some(id) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };
    let Some(kind) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };
    let Some(url_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
        return false;
    };
    let Some(filename_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
        return false;
    };
    let Some(dest_dir_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
        return false;
    };
    let Some(final_dir_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
        return false;
    };
    let hard_link_c = if format_version >= 63 {
        let Some(v) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
            return false;
        };
        Some(v)
    } else {
        None
    };
    let meta_name_c = if format_version >= 64 && format_version != 65 {
        let Some(v) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
            return false;
        };
        Some(v)
    } else {
        None
    };
    let Some(queued_filename_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) })
    else {
        return false;
    };
    let Some(name_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
        return false;
    };
    let Some(category_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
        return false;
    };

    let Some(prio_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut prc = ScanCursor(&prio_line);
    let (Some(priority), Some(raw_post_stage), Some(delete_paused), Some(many_dupe_files)) =
        (prc.i32(false), prc.i32(true), prc.i32(true), prc.i32(true))
    else {
        return false;
    };
    let feed_id = if format_version >= 54 {
        let Some(fid) = prc.i32(true) else {
            return false;
        };
        fid
    } else {
        0
    };
    let (has_post_info, post_stage) = if raw_post_stage > 0 {
        (1, normalize_post_stage(raw_post_stage, format_version, pt_finished))
    } else {
        (0, 0)
    };

    let Some(st_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut stc = ScanCursor(&st_line);
    let (Some(par_status), Some(unpack_status), Some(move_status), Some(par_rename_status)) =
        (stc.i32(false), stc.i32(true), stc.i32(true), stc.i32(true))
    else {
        return false;
    };
    let rar_rename_status = if format_version >= 58 {
        let Some(v) = stc.i32(true) else { return false };
        v
    } else {
        0
    };
    let direct_rename_status = if format_version >= 60 {
        let Some(v) = stc.i32(true) else { return false };
        v
    } else {
        0
    };
    let (Some(delete_status), Some(mark_status), Some(url_status)) =
        (stc.i32(true), stc.i32(true), stc.i32(true))
    else {
        return false;
    };

    let Some(pause_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut psc = ScanCursor(&pause_line);
    let (Some(unpack_cleaned_up_disk), Some(health_paused), Some(add_url_paused)) =
        (psc.i32(false), psc.i32(true), psc.i32(true))
    else {
        return false;
    };

    let Some(fc_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut fcc = ScanCursor(&fc_line);
    let (Some(file_count), Some(parked_file_count)) = (fcc.i32(false), fcc.i32(true)) else {
        return false;
    };
    let message_count = if format_version >= 52 {
        let Some(mc) = fcc.i32(true) else { return false };
        mc
    } else {
        0
    };

    let Some(tm_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut tmc = ScanCursor(&tm_line);
    let (Some(min_time), Some(max_time)) = (tmc.i32(false), tmc.i32(true)) else {
        return false;
    };

    let mut flags: c_int = 0;
    let mut par_full = 0;
    let mut force_par_full = 0;
    let mut force_repair = 0;
    let mut extra_par_blocks = 0;
    if format_version >= 51 {
        flags |= 1;
        let Some(pf_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        let mut pfc = ScanCursor(&pf_line);
        let (Some(pf), Some(fpf), Some(fr)) = (pfc.i32(false), pfc.i32(true), pfc.i32(true)) else {
            return false;
        };
        par_full = pf;
        force_par_full = fpf;
        force_repair = fr;
        if format_version >= 55 {
            let Some(epb) = pfc.i32(true) else { return false };
            extra_par_blocks = epb;
        }
    }

    let Some(hash_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut hsc = ScanCursor(&hash_line);
    let (Some(full_content_hash), Some(filtered_content_hash)) =
        (hsc.u32(false), hsc.u32(true))
    else {
        return false;
    };

    let Some(sz1_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut sz1c = ScanCursor(&sz1_line);
    let (Some(h1), Some(l1), Some(h2), Some(l2), Some(h3), Some(l3)) = (
        sz1c.u32(false),
        sz1c.u32(true),
        sz1c.u32(true),
        sz1c.u32(true),
        sz1c.u32(true),
        sz1c.u32(true),
    ) else {
        return false;
    };
    let size = join_i64(h1, l1);
    let success_size = join_i64(h2, l2);
    let failed_size = join_i64(h3, l3);

    let Some(sz2_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut sz2c = ScanCursor(&sz2_line);
    let (Some(ph1), Some(pl1), Some(ph2), Some(pl2), Some(ph3), Some(pl3)) = (
        sz2c.u32(false),
        sz2c.u32(true),
        sz2c.u32(true),
        sz2c.u32(true),
        sz2c.u32(true),
        sz2c.u32(true),
    ) else {
        return false;
    };
    let par_size = join_i64(ph1, pl1);
    let par_success_size = join_i64(ph2, pl2);
    let par_failed_size = join_i64(ph3, pl3);

    let Some(art_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut artc = ScanCursor(&art_line);
    let (Some(total_articles), Some(success_articles), Some(failed_articles)) =
        (artc.i32(false), artc.i32(true), artc.i32(true))
    else {
        return false;
    };

    let Some(dupe_key_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
        return false;
    };

    let Some(dupe_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut dpc = ScanCursor(&dupe_line);
    let (Some(dupe_mode), Some(dupe_score)) = (dpc.i32(false), dpc.i32(true)) else {
        return false;
    };
    let dupe_hint = if format_version >= 61 {
        let Some(dh) = dpc.i32(true) else { return false };
        dh
    } else {
        0
    };

    let mut downloaded_size = 0i64;
    let mut download_sec = 0;
    let mut post_total_sec = 0;
    let mut par_sec = 0;
    let mut repair_sec = 0;
    let mut unpack_sec = 0;
    if format_version >= 48 {
        flags |= 2;
        let Some(dl_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        let mut dlc = ScanCursor(&dl_line);
        let (
            Some(dh4),
            Some(dl4),
            Some(dsec),
            Some(ptsec),
            Some(psec),
            Some(rsec),
            Some(usec),
        ) = (
            dlc.u32(false),
            dlc.u32(true),
            dlc.i32(true),
            dlc.i32(true),
            dlc.i32(true),
            dlc.i32(true),
            dlc.i32(true),
        )
        else {
            return false;
        };
        downloaded_size = join_i64(dh4, dl4);
        download_sec = dsec;
        post_total_sec = ptsec;
        par_sec = psec;
        repair_sec = rsec;
        unpack_sec = usec;
    }

    let Some(comp_count) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };
    struct CompHold {
        id: c_int,
        status: c_int,
        crc: u32,
        par_file: c_int,
        h16k: Vec<u8>,
        pset: Vec<u8>,
        fname: Vec<u8>,
        orig: Vec<u8>,
    }
    let mut comp_holds = Vec::with_capacity(comp_count.clamp(0, 1024) as usize);
    for _ in 0..comp_count {
        let Some(mut line_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 10240) }) else {
            return false;
        };
        let mut cf_id = 0;
        let mut cf_status = 0;
        let mut cf_crc = 0u32;
        let mut cf_par = 0;
        let mut cf_h16k: *const c_char = std::ptr::null();
        let mut cf_pset: *const c_char = std::ptr::null();
        let mut cf_fname: *const c_char = std::ptr::null();
        if unsafe {
            parse_completed_file_line(
                line_c.as_mut_ptr().cast(),
                format_version,
                &mut cf_id,
                &mut cf_status,
                &mut cf_crc,
                &mut cf_par,
                &mut cf_h16k,
                &mut cf_pset,
                &mut cf_fname,
            )
        } == 0
        {
            return false;
        }
        let h_slice = unsafe { cstr_to_bytes(cf_h16k) };
        let mut h_c = Vec::with_capacity(h_slice.len() + 1);
        h_c.extend_from_slice(h_slice);
        h_c.push(0);

        let p_slice = unsafe { cstr_to_bytes(cf_pset) };
        let mut p_c = Vec::with_capacity(p_slice.len() + 1);
        p_c.extend_from_slice(p_slice);
        p_c.push(0);

        let (fname_c, orig_c) = if format_version >= 62 {
            let Some(f_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
                return false;
            };
            let Some(o_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
                return false;
            };
            (f_c, o_c)
        } else {
            let f_slice = unsafe { cstr_to_bytes(cf_fname) };
            let mut f_c = Vec::with_capacity(f_slice.len() + 1);
            f_c.extend_from_slice(f_slice);
            f_c.push(0);
            (f_c, vec![0])
        };
        comp_holds.push(CompHold {
            id: cf_id,
            status: cf_status,
            crc: cf_crc,
            par_file: cf_par,
            h16k: h_c,
            pset: p_c,
            fname: fname_c,
            orig: orig_c,
        });
    }
    let completed: Vec<NzbgetRsDiskCompletedFile> = comp_holds
        .iter()
        .map(|h| NzbgetRsDiskCompletedFile {
            id: h.id,
            status: h.status,
            crc: h.crc,
            par_file: h.par_file,
            hash16k: h.h16k.as_ptr().cast(),
            par_set_id: h.pset.as_ptr().cast(),
            filename: h.fname.as_ptr().cast(),
            origname: h.orig.as_ptr().cast(),
        })
        .collect();

    let Some(param_hdr) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut prmc = ScanCursor(&param_hdr);
    let Some(parameter_count) = prmc.i32(false) else {
        return false;
    };
    let mut skip_script_processing = 0;
    let mut skip_disk_write = 0;
    let mut desired_server_id = 0;
    if let Some(ssp) = prmc.i32(true) {
        let (Some(sdw), Some(dsi)) = (prmc.i32(true), prmc.i32(true)) else {
            return false;
        };
        skip_script_processing = ssp;
        skip_disk_write = sdw;
        desired_server_id = dsi;
        flags |= 4;
    }
    let mut param_holds: Vec<(Vec<u8>, Vec<u8>)> =
        Vec::with_capacity(parameter_count.clamp(0, 256) as usize);
    for _ in 0..parameter_count {
        let Some(p_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 10240) }) else {
            return false;
        };
        if let Some(eq_pos) = p_line.iter().position(|&b| b == b'=') {
            let mut k = p_line[..eq_pos].to_vec();
            k.push(0);
            let mut v = p_line[eq_pos + 1..].to_vec();
            v.push(0);
            param_holds.push((k, v));
        }
    }
    let params: Vec<NzbgetRsDiskParam> = param_holds
        .iter()
        .map(|(k, v)| NzbgetRsDiskParam {
            name: k.as_ptr().cast(),
            value: v.as_ptr().cast(),
        })
        .collect();

    let Some(script_count) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };
    let mut script_holds: Vec<(c_int, Vec<u8>)> =
        Vec::with_capacity(script_count.clamp(0, 256) as usize);
    for _ in 0..script_count {
        let Some(s_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 10240) }) else {
            return false;
        };
        if let Some(comma_pos) = s_line.iter().position(|&b| b == b',') {
            let mut st = c_atoi(&s_line[..comma_pos]);
            if st > 1 && format_version < 25 {
                st -= 1;
            }
            let mut sname = s_line[comma_pos + 1..].to_vec();
            sname.push(0);
            script_holds.push((st, sname));
        }
    }
    let scripts: Vec<NzbgetRsDiskScriptStatus> = script_holds
        .iter()
        .map(|(st, sname)| NzbgetRsDiskScriptStatus {
            status: *st,
            name: sname.as_ptr().cast(),
        })
        .collect();

    let Some(stats) = (unsafe { read_server_stats(infile_ctx, read_line_cb) }) else {
        return false;
    };

    if format_version < 52 {
        let Some(log_count) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
            return false;
        };
        for _ in 0..log_count {
            if (unsafe { read_line_bytes(infile_ctx, read_line_cb, 10240) }).is_none() {
                return false;
            }
        }
    }

    let Some(stub_count) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };
    if format_version < 56 {
        flags |= 8;
    }
    let cap = stub_count.clamp(0, 1024) as usize;
    let mut files = Vec::with_capacity(cap);
    let mut file_times = Vec::with_capacity(cap);
    for _ in 0..stub_count {
        let Some(f_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        let mut fc = ScanCursor(&f_line);
        let (fid, paused, ftime, extra_priority) = if format_version >= 56 {
            let (Some(i), Some(p), Some(ep)) = (fc.i32(false), fc.i32(true), fc.i32(true)) else {
                return false;
            };
            (i, p, 0, ep)
        } else {
            let (Some(i), Some(p), Some(t), Some(ep)) =
                (fc.i32(false), fc.i32(true), fc.i32(true), fc.i32(true))
            else {
                return false;
            };
            (i, p, t, ep)
        };
        files.push(NzbgetRsDiskFileStub {
            id: fid,
            paused,
            extra_priority,
        });
        file_times.push(ftime);
    }

    let inp = NzbgetRsDiskNzbInfoInput {
        id,
        kind,
        url: url_c.as_ptr().cast(),
        filename: filename_c.as_ptr().cast(),
        dest_dir: dest_dir_c.as_ptr().cast(),
        final_dir: final_dir_c.as_ptr().cast(),
        hard_link_path: hard_link_c
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr().cast()),
        meta_name: meta_name_c
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr().cast()),
        queued_filename: queued_filename_c.as_ptr().cast(),
        name: if name_c.len() > 1 {
            name_c.as_ptr().cast()
        } else {
            std::ptr::null()
        },
        category: category_c.as_ptr().cast(),
        priority,
        has_post_info,
        post_stage,
        delete_paused,
        many_dupe_files,
        feed_id,
        par_status,
        unpack_status,
        move_status,
        par_rename_status,
        rar_rename_status,
        direct_rename_status,
        delete_status,
        mark_status,
        url_status,
        unpack_cleaned_up_disk,
        health_paused,
        add_url_paused,
        file_count,
        parked_file_count,
        message_count,
        min_time,
        max_time,
        par_full,
        force_par_full,
        force_repair,
        extra_par_blocks,
        full_content_hash,
        filtered_content_hash,
        size,
        success_size,
        failed_size,
        par_size,
        par_success_size,
        par_failed_size,
        total_articles,
        success_articles,
        failed_articles,
        dupe_key: dupe_key_c.as_ptr().cast(),
        dupe_mode,
        dupe_score,
        dupe_hint,
        downloaded_size,
        download_sec,
        post_total_sec,
        par_sec,
        repair_sec,
        unpack_sec,
        skip_script_processing,
        skip_disk_write,
        desired_server_id,
    };

    unsafe {
        on_loaded_cb(
            target_ctx,
            &inp,
            flags,
            completed.as_ptr(),
            completed.len(),
            params.as_ptr(),
            params.len(),
            scripts.as_ptr(),
            scripts.len(),
            stats.as_ptr(),
            stats.len(),
            files.as_ptr(),
            file_times.as_ptr(),
            files.len(),
        );
    }
    true
}

/// Parses `DiskState::LoadFileState` via `read_line_cb`.
/// `flags` bitmask passed to `on_summary_cb`:
/// - `1`: `decoded_file_size` present (`format_version >= 8` or `< 8` `#decoded` trailer)
/// - `2`: non-empty `filename` present (`format_version >= 4`)
/// - `4`: `hash16k` and `par_file` present (`format_version >= 5`)
/// - `8`: `par_set_id` present (`format_version >= 6`)
/// - `16`: `output_initialized` updated (`< 8` trailer or `DirectWrite` reconciliation)
#[allow(clippy::too_many_arguments)]
pub unsafe fn load_file_state(
    infile_ctx: *mut std::ffi::c_void,
    read_line_cb: ReadLineFn,
    format_version: c_int,
    completed: c_int,
    file_id: c_int,
    has_articles: c_int,
    articles_len: c_int,
    has_options: c_int,
    direct_write: c_int,
    force_direct_write: c_int,
    cur_output_initialized: c_int,
    sep: u8,
    temp_dir: &[u8],
    dest_dir: &[u8],
    cur_filename: &[u8],
    cur_output_filename: &[u8],
    target_ctx: *mut std::ffi::c_void,
    article_cb: unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        op: c_int,
        idx: c_int,
        status: c_int,
        seg_offset: i64,
        seg_size: c_int,
        crc: u32,
        fallback_round: c_int,
        result_filename: *const c_char,
    ) -> c_int,
    on_summary_cb: unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        fields: *const NzbgetRsDiskFileStateInput,
        flags: c_int,
        completed_articles: c_int,
        new_output_filename: *const c_char,
        stats: *const NzbgetRsDiskServerStat,
        stats_len: usize,
    ),
) -> bool {
    let Some(art_hdr) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut ahc = ScanCursor(&art_hdr);
    let (Some(success_articles), Some(failed_articles)) = (ahc.i32(false), ahc.i32(true)) else {
        return false;
    };

    let Some(sz_hdr) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
        return false;
    };
    let mut szc = ScanCursor(&sz_hdr);
    let (Some(h1), Some(l1), Some(h2), Some(l2), Some(h3), Some(l3)) = (
        szc.u32(false),
        szc.u32(true),
        szc.u32(true),
        szc.u32(true),
        szc.u32(true),
        szc.u32(true),
    ) else {
        return false;
    };
    let remaining_size = join_i64(h1, l1);
    let success_size = join_i64(h2, l2);
    let failed_size = join_i64(h3, l3);

    let mut flags: c_int = 0;
    let mut decoded_file_size = 0i64;
    if format_version >= 8 {
        let Some(dec_hdr) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        let mut dc = ScanCursor(&dec_hdr);
        let (Some(dh), Some(dl)) = (dc.u32(false), dc.u32(true)) else {
            return false;
        };
        decoded_file_size = join_i64(dh, dl);
        flags |= 1;
    }

    let filename_c = if format_version >= 4 {
        let Some(v) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        if v.len() > 1 {
            flags |= 2;
        }
        v
    } else {
        vec![0]
    };

    let mut hash16k_c = vec![0];
    let mut par_set_id_c = vec![0];
    let mut par_file = 0;
    if format_version >= 5 {
        let Some(h_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        hash16k_c = h_c;
        flags |= 4;
        if format_version >= 6 {
            let Some(p_c) = (unsafe { read_line_cstr(infile_ctx, read_line_cb, 1024) }) else {
                return false;
            };
            par_set_id_c = p_c;
            flags |= 8;
        }
        let Some(pf) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
            return false;
        };
        par_file = pf;
    }

    let Some(stats) = (unsafe { read_server_stats(infile_ctx, read_line_cb) }) else {
        return false;
    };

    let Some(size) = (unsafe { scan_one_i32(infile_ctx, read_line_cb) }) else {
        return false;
    };
    if !validate_article_count(size, has_articles != 0, articles_len) {
        return false;
    }

    let comp_bool = completed != 0;
    let dw_bool = direct_write != 0;
    let fdw_bool = force_direct_write != 0;
    let mut completed_articles = 0;

    for i in 0..size {
        let meta = unsafe { article_cb(target_ctx, 0, i, 0, 0, 0, 0, 0, std::ptr::null()) };
        let part_number = meta >> 1;
        let has_result_filename = (meta & 1) != 0;

        let Some(art_line) = (unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) }) else {
            return false;
        };
        let mut ac = ScanCursor(&art_line);
        let mut fields = 0;
        let mut status_int = 0;
        let mut segment_offset = 0i64;
        let mut segment_size = 0;
        let mut crc = 0u32;
        let mut staged_fallback = 0;
        if let Some(v0) = ac.i32(false) {
            status_int = v0;
            fields = 1;
            if format_version >= 2 {
                if let Some(v1) = ac.i64(true) {
                    segment_offset = v1;
                    fields = 2;
                    if let Some(v2) = ac.i32(true) {
                        segment_size = v2;
                        fields = 3;
                        if let Some(v3) = ac.u32(true) {
                            crc = v3;
                            fields = 4;
                            if let Some(v4) = ac.i32(true) {
                                staged_fallback = v4;
                                fields = 5;
                            }
                        }
                    }
                }
            }
        }

        let staged_exists = need_staged_file_check(
            comp_bool,
            format_version,
            status_int,
            2, // ArticleInfo::aiFinished
            segment_offset,
            segment_size,
            staged_fallback,
            part_number,
            dw_bool,
            fdw_bool,
        ) && {
            let p = diskstate_format_staged_part_path(temp_dir, sep, file_id, part_number);
            crate::fsops::posix_stat(&p) == 1
        };

        let ev = eval_article_load_state(
            format_version,
            comp_bool,
            fields,
            status_int,
            segment_offset,
            segment_size,
            staged_fallback,
            part_number,
            staged_exists,
            dw_bool,
            fdw_bool,
            has_result_filename,
            completed_articles,
            size,
        );
        if ev.valid == 0 {
            return false;
        }
        completed_articles = ev.completed_articles;

        let mut res_fn_c;
        let res_ptr = if ev.need_result_filename != 0 {
            res_fn_c = diskstate_format_staged_part_path(temp_dir, sep, file_id, part_number);
            res_fn_c.push(0);
            res_fn_c.as_ptr().cast::<c_char>()
        } else {
            std::ptr::null()
        };
        unsafe {
            article_cb(
                target_ctx,
                1,
                i,
                ev.status,
                ev.segment_offset,
                ev.segment_size,
                crc,
                ev.dupe_fallback_round,
                res_ptr,
            );
        }
    }

    let mut out_init_bool = cur_output_initialized != 0;
    if format_version < 8 {
        while let Some(trailer) = unsafe { read_line_bytes(infile_ctx, read_line_cb, 1024) } {
            let (kind, dec_size, out_init) = parse_legacy_file_state_trailer(&trailer);
            if kind == 1 {
                decoded_file_size = dec_size;
                flags |= 1;
            } else if kind == 2 {
                out_init_bool = out_init != 0;
                flags |= 16;
            }
        }
    }

    let mut new_out_fn_c: Option<Vec<u8>> = None;
    if has_options != 0 && (dw_bool || fdw_bool) {
        let eff_filename = if (flags & 2) != 0 {
            &filename_c[..filename_c.len() - 1]
        } else {
            cur_filename
        };
        let has_own_out =
            !cur_output_filename.is_empty() && crate::fsops::posix_stat(cur_output_filename) == 1;
        let out_tmp_p = if !dest_dir.is_empty() {
            crate::artwritercore::article_writer_format_dest_file_path(
                dest_dir,
                sep,
                format!("{file_id}.out.tmp").as_bytes(),
            )
        } else {
            Vec::new()
        };
        let has_out_tmp = !out_tmp_p.is_empty() && crate::fsops::posix_stat(&out_tmp_p) == 1;
        let comp_dest_p = if (comp_bool || fdw_bool)
            && !dest_dir.is_empty()
            && !eff_filename.is_empty()
        {
            crate::artwritercore::article_writer_format_dest_file_path(dest_dir, sep, eff_filename)
        } else {
            Vec::new()
        };
        let has_comp_dest = !comp_dest_p.is_empty() && crate::fsops::posix_stat(&comp_dest_p) == 1;

        let (new_init, pick_kind) = reconcile_direct_write_output(
            out_init_bool,
            has_own_out,
            has_out_tmp,
            has_comp_dest,
            success_articles,
            comp_bool,
            cur_output_filename.is_empty(),
        );
        out_init_bool = new_init;
        flags |= 16;
        if pick_kind == 1 {
            let mut p = out_tmp_p;
            p.push(0);
            new_out_fn_c = Some(p);
        } else if pick_kind == 2 {
            let mut p = comp_dest_p;
            p.push(0);
            new_out_fn_c = Some(p);
        }
    }

    let state_fields = NzbgetRsDiskFileStateInput {
        success_articles,
        failed_articles,
        remaining_size,
        success_size,
        failed_size,
        filename: filename_c.as_ptr().cast(),
        hash16k: hash16k_c.as_ptr().cast(),
        par_set_id: par_set_id_c.as_ptr().cast(),
        par_file,
        decoded_file_size,
        output_initialized: c_int::from(out_init_bool),
    };

    unsafe {
        on_summary_cb(
            target_ctx,
            &state_fields,
            flags,
            completed_articles,
            new_out_fn_c
                .as_ref()
                .map_or(std::ptr::null(), |v| v.as_ptr().cast()),
            stats.as_ptr(),
            stats.len(),
        );
    }
    true
}

const DISKSTATE_QUEUE_VERSION: i32 = 64;
const DISKSTATE_QUEUE_LEGACY_VERSION: i32 = 66;
const DISKSTATE_FILE_VERSION: i32 = 7;
const DISKSTATE_FILE_LEGACY_VERSION: i32 = 9;
const DISKSTATE_STATS_VERSION: i32 = 4;
const DISKSTATE_FEEDS_VERSION: i32 = 3;
const DUPESTATE_TRAILER: &[u8] = b"#dupestate 1";
const UNPACKSTATE_TRAILER: &[u8] = b"#unpackstate 1";

#[repr(C)]
#[derive(Copy, Clone)]
pub struct NzbgetRsServerMatchItem {
    pub id: c_int,
    pub port: c_int,
    pub name: *const c_char,
    pub host: *const c_char,
    pub user: *const c_char,
}

#[repr(C)]
pub struct NzbgetRsDiskNzbExport {
    pub hist_id: c_int,
    pub hist_kind: c_int,
    pub hist_time: c_int,
    pub changed: c_int,
    pub info: NzbgetRsDiskNzbInfoInput,
    pub completed: *const NzbgetRsDiskCompletedFile,
    pub completed_len: usize,
    pub params: *const NzbgetRsDiskParam,
    pub params_len: usize,
    pub scripts: *const NzbgetRsDiskScriptStatus,
    pub scripts_len: usize,
    pub stats: *const NzbgetRsDiskServerStat,
    pub stats_len: usize,
    pub files: *const NzbgetRsDiskFileStub,
    pub files_len: usize,
    pub dupe_recovered_articles: c_int,
    pub dupe_recovered_bytes: i64,
    pub dupe_recovered_holes: c_int,
    pub jobs: *const NzbgetRsDiskRepairJob,
    pub jobs_len: usize,
    pub direct_unpack_status: c_int,
    pub unpack_tried: c_int,
    pub par_repaired: c_int,
    pub last_unpack_status: c_int,
    pub archives: *const *const c_char,
    pub archives_len: usize,
}

#[repr(C)]
pub struct NzbgetRsDiskFileExport {
    pub file_id: c_int,
    pub has_articles: c_int,
    pub articles_len_count: c_int,
    pub force_direct_write: c_int,
    pub info: NzbgetRsDiskFileInfoInput,
    pub groups: *const *const c_char,
    pub groups_len: usize,
    pub articles: *const NzbgetRsDiskArticleInfo,
    pub articles_len: usize,
    pub state: NzbgetRsDiskFileStateInput,
    pub stats: *const NzbgetRsDiskServerStat,
    pub stats_len: usize,
    pub article_states: *const NzbgetRsDiskArticleState,
    pub article_states_len: usize,
}

pub type DiskLogFn = unsafe extern "C" fn(ctx: *mut c_void, level: c_int, msg: *const c_char);
pub type DiskCtrlFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    cmd: c_int,
    a1: c_int,
    a2: c_int,
    a3: i64,
    s1: *const c_char,
) -> c_int;
pub type DiskNzbSinkFn =
    unsafe extern "C" fn(sink_ctx: *mut c_void, exp: *const NzbgetRsDiskNzbExport);
pub type DiskExportNzbFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    source: c_int,
    idx: usize,
    sink_ctx: *mut c_void,
    sink_fn: DiskNzbSinkFn,
);
pub type DiskApplyNzbFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    mode: c_int,
    inp: *const NzbgetRsDiskNzbInfoInput,
    flags: c_int,
    completed: *const NzbgetRsDiskCompletedFile,
    completed_len: usize,
    params: *const NzbgetRsDiskParam,
    params_len: usize,
    scripts: *const NzbgetRsDiskScriptStatus,
    scripts_len: usize,
    stats: *const NzbgetRsDiskServerStat,
    stats_len: usize,
    files: *const NzbgetRsDiskFileStub,
    file_times: *const c_int,
    files_len: usize,
    job: *const NzbgetRsDiskRepairJob,
);
pub type DiskFileSinkFn =
    unsafe extern "C" fn(sink_ctx: *mut c_void, exp: *const NzbgetRsDiskFileExport);
pub type DiskExportFileFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    mode: c_int,
    sink_ctx: *mut c_void,
    sink_fn: DiskFileSinkFn,
);
pub type DiskApplyFileFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    mode: c_int,
    idx_or_flags: c_int,
    extra_i32: c_int,
    seg_offset: i64,
    crc: u32,
    str_arg: *const c_char,
    finfo: *const NzbgetRsDiskFileInfoInput,
    art_info: *const NzbgetRsDiskArticleInfo,
    fstate: *const NzbgetRsDiskFileStateInput,
    stats: *const NzbgetRsDiskServerStat,
    stats_len: usize,
) -> c_int;
pub type DiskAuxSinkFn = unsafe extern "C" fn(
    sink_ctx: *mut c_void,
    kind: c_int,
    a1: c_int,
    a2: c_int,
    u1: u32,
    s1: *const c_char,
    srv_items: *const NzbgetRsServerMatchItem,
    srv_len: usize,
    vol_items: *const NzbgetRsDiskVolumeStatInput,
    vol_len: usize,
);
pub type DiskAuxFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    u1: u32,
    s1: *const c_char,
    s2: *const c_char,
    vol_item: *const NzbgetRsDiskVolumeStatInput,
    sink_ctx: *mut c_void,
    sink_fn: Option<DiskAuxSinkFn>,
);

#[repr(C)]
pub struct NzbgetRsDiskStateEnv {
    pub ctx: *mut c_void,
    pub queue_dir: *const c_char,
    pub temp_dir: *const c_char,
    pub line_ending: *const c_char,
    pub time_str: *const c_char,
    pub text: *const c_char,
    pub sep: c_int,
    pub has_options: c_int,
    pub flush_queue: c_int,
    pub direct_write: c_int,
    pub continue_partial: c_int,
    pub pt_finished: c_int,
    pub ns_running: c_int,
    pub ns_none: c_int,
    pub a1: c_int,
    pub a2: c_int,
    pub a3: c_int,
    pub a4: c_int,
    pub now: i64,
    pub out_flag: *mut c_int,
    pub log_cb: Option<DiskLogFn>,
    pub ctrl_cb: Option<DiskCtrlFn>,
    pub export_nzb_cb: Option<DiskExportNzbFn>,
    pub apply_nzb_cb: Option<DiskApplyNzbFn>,
    pub export_file_cb: Option<DiskExportFileFn>,
    pub apply_file_cb: Option<DiskApplyFileFn>,
    pub aux_cb: Option<DiskAuxFn>,
}

fn bytes_to_cstring_trunc(raw: &[u8]) -> std::ffi::CString {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    unsafe { std::ffi::CString::from_vec_unchecked(raw[..end].to_vec()) }
}

unsafe fn emit_log(env: &NzbgetRsDiskStateEnv, level: c_int, msg: &[u8]) {
    if let Some(cb) = env.log_cb {
        let cstr = bytes_to_cstring_trunc(msg);
        unsafe { cb(env.ctx, level, cstr.as_ptr()) };
    }
}

unsafe fn emit_log_parts(env: &NzbgetRsDiskStateEnv, level: c_int, parts: &[&[u8]]) {
    let joined: Vec<u8> = parts.concat();
    unsafe { emit_log(env, level, &joined) };
}

unsafe fn call_ctrl(
    env: &NzbgetRsDiskStateEnv,
    cmd: c_int,
    a1: c_int,
    a2: c_int,
    a3: i64,
    s1: *const c_char,
) -> c_int {
    if let Some(cb) = env.ctrl_cb {
        unsafe { cb(env.ctx, cmd, a1, a2, a3, s1) }
    } else {
        0
    }
}

struct RsStateReader {
    reader: std::io::BufReader<std::fs::File>,
    raw_buf: Vec<u8>,
    cstr_buf: Vec<u8>,
}

impl RsStateReader {
    fn open(path_c: &std::ffi::CStr) -> Result<Self, i32> {
        let fd = crate::fsops::open_cloexec_fd(path_c, 0, 0, 1, 2);
        if fd == -1 {
            return Err(crate::fsops::last_errno());
        }
        let file = unsafe { std::os::unix::io::FromRawFd::from_raw_fd(fd) };
        Ok(Self {
            reader: std::io::BufReader::new(file),
            raw_buf: Vec::with_capacity(1024),
            cstr_buf: Vec::with_capacity(1024),
        })
    }

    fn read_line(&mut self, max_size: usize) -> Option<&[u8]> {
        use std::io::BufRead;
        if max_size == 0 {
            return None;
        }
        let cap = max_size - 1;
        self.raw_buf.clear();
        if cap == 0 {
            return match self.reader.fill_buf() {
                Ok([]) | Err(_) => None,
                Ok(_) => Some(&[]),
            };
        }
        let mut saw_any = false;
        let mut saw_newline = false;
        while self.raw_buf.len() < cap {
            let avail = match self.reader.fill_buf() {
                Ok([]) => break,
                Ok(buf) => buf,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            };
            saw_any = true;
            let rem = cap - self.raw_buf.len();
            let window = &avail[..avail.len().min(rem)];
            if let Some(nl_pos) = window.iter().position(|&b| b == b'\n') {
                self.raw_buf.extend_from_slice(&window[..=nl_pos]);
                self.reader.consume(nl_pos + 1);
                saw_newline = true;
                break;
            }
            let take = window.len();
            self.raw_buf.extend_from_slice(window);
            self.reader.consume(take);
        }
        if !saw_any {
            return None;
        }
        if !saw_newline {
            loop {
                let avail = match self.reader.fill_buf() {
                    Ok([]) => break,
                    Ok(buf) => buf,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                if let Some(nl_pos) = avail.iter().position(|&b| b == b'\n') {
                    self.reader.consume(nl_pos + 1);
                    break;
                }
                let take = avail.len();
                self.reader.consume(take);
            }
        }
        let nul_len = self
            .raw_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.raw_buf.len());
        if nul_len > 0 {
            Some(&self.raw_buf[..nul_len - 1])
        } else {
            Some(&[])
        }
    }

    fn scan_i32(&mut self) -> Option<i32> {
        let line = self.read_line(1024)?;
        ScanCursor(line).i32(false)
    }

    fn as_ctx(&mut self) -> *mut c_void {
        (self as *mut Self).cast()
    }

    unsafe extern "C" fn read_line_cb(ctx: *mut c_void, max_size: c_int) -> *const c_char {
        let this = unsafe { &mut *ctx.cast::<Self>() };
        let eff_max = if max_size > 0 {
            max_size as usize
        } else {
            10240
        };
        let Some(slice) = this.read_line(eff_max) else {
            return std::ptr::null();
        };
        let slice_vec = slice.to_vec();
        this.cstr_buf.clear();
        this.cstr_buf.extend_from_slice(&slice_vec);
        this.cstr_buf.push(0);
        this.cstr_buf.as_ptr().cast()
    }
}

struct RsStateFile {
    dest: Vec<u8>,
    temp: Vec<u8>,
    dest_c: std::ffi::CString,
    temp_c: std::ffi::CString,
    sep: u8,
    format_version: i32,
    read_version: i32,
    transactional: bool,
    flush: bool,
    file_version: i32,
}

impl RsStateFile {
    fn new(
        qdir: &[u8],
        sep: u8,
        filename: &[u8],
        format_version: i32,
        transactional: bool,
        read_version: i32,
    ) -> Self {
        let dest =
            crate::artwritercore::article_writer_format_dest_file_path(qdir, sep, filename);
        let temp = if transactional {
            diskstate_format_new_path(&dest)
        } else {
            dest.clone()
        };
        let dest_c = bytes_to_cstring_trunc(&dest);
        let temp_c = bytes_to_cstring_trunc(&temp);
        Self {
            dest,
            temp,
            dest_c,
            temp_c,
            sep,
            format_version,
            read_version: if read_version != 0 {
                read_version
            } else {
                format_version
            },
            transactional,
            flush: true,
            file_version: 0,
        }
    }

    fn discard(&self) {
        crate::fsops::delete_file(&self.dest_c);
        if self.transactional {
            crate::fsops::delete_file(&self.temp_c);
        }
    }

    fn file_exists(&self) -> bool {
        crate::fsops::posix_stat(&self.dest) == 1
            || (self.transactional && crate::fsops::posix_stat(&self.temp) == 1)
    }

    unsafe fn begin_write(
        &self,
        env: &NzbgetRsDiskStateEnv,
    ) -> Option<std::io::BufWriter<std::fs::File>> {
        let fd = crate::fsops::open_cloexec_fd(&self.temp_c, 2, 0, 1, 2);
        if fd == -1 {
            let err = crate::fsops::error_message(crate::fsops::last_errno());
            unsafe {
                emit_log_parts(
                    env,
                    4,
                    &[
                        b"Error saving diskstate: Could not create file ",
                        &self.temp,
                        b": ",
                        &err,
                    ],
                );
            }
            return None;
        }
        let file = unsafe { std::os::unix::io::FromRawFd::from_raw_fd(fd) };
        let mut w = std::io::BufWriter::new(file);
        let _ = writeln!(w, "nzbget diskstate file version {}", self.format_version);
        Some(w)
    }

    unsafe fn finish_write(
        &self,
        env: &NzbgetRsDiskStateEnv,
        mut w: std::io::BufWriter<std::fs::File>,
        flush_queue: bool,
    ) -> bool {
        let _ = w.flush();
        if !self.transactional {
            drop(w);
            return true;
        }

        if self.flush && flush_queue {
            unsafe {
                emit_log_parts(
                    env,
                    0,
                    &[
                        b"Flushing data for file ",
                        crate::nserv::base_file_name(&self.temp),
                    ],
                );
            }
            let mut errmsg = Vec::new();
            let fd = std::os::unix::io::AsRawFd::as_raw_fd(w.get_ref());
            if !crate::fsops::flush_file_buffers(fd, &mut errmsg) {
                unsafe {
                    emit_log_parts(
                        env,
                        3,
                        &[
                            b"Could not flush file ",
                            &self.temp,
                            b" into disk: ",
                            &errmsg,
                        ],
                    );
                }
            }
        }

        drop(w);

        crate::fsops::delete_file(&self.dest_c);
        if !crate::fsops::move_file(&self.temp_c, &self.dest_c) {
            let err = crate::fsops::error_message(crate::fsops::last_errno());
            unsafe {
                emit_log_parts(
                    env,
                    4,
                    &[
                        b"Error saving diskstate: Could not rename file ",
                        &self.temp,
                        b" to ",
                        &self.dest,
                        b": ",
                        &err,
                    ],
                );
            }
            return false;
        }

        if self.flush && flush_queue {
            unsafe {
                emit_log_parts(
                    env,
                    0,
                    &[
                        b"Flushing directory for file ",
                        crate::nserv::base_file_name(&self.dest),
                    ],
                );
            }
            let mut errmsg = Vec::new();
            if !crate::fsops::flush_dir_buffers(&self.dest_c, self.sep, &mut errmsg) {
                unsafe {
                    emit_log_parts(
                        env,
                        3,
                        &[
                            b"Could not flush directory buffers for file ",
                            &self.dest,
                            b" into disk: ",
                            &errmsg,
                        ],
                    );
                }
            }
        }

        true
    }

    unsafe fn begin_read(&mut self, env: &NzbgetRsDiskStateEnv) -> Option<RsStateReader> {
        let dest_exists = crate::fsops::posix_stat(&self.dest) == 1;
        let temp_exists = crate::fsops::posix_stat(&self.temp) == 1;
        if self.transactional && dest_exists && temp_exists {
            crate::fsops::delete_file(&self.temp_c);
        } else if !dest_exists && temp_exists {
            unsafe {
                emit_log_parts(
                    env,
                    3,
                    &[
                        b"Restoring diskstate file ",
                        crate::nserv::base_file_name(&self.dest),
                        b" from ",
                        crate::nserv::base_file_name(&self.temp),
                    ],
                );
            }
            if !crate::fsops::move_file(&self.temp_c, &self.dest_c) {
                let err = crate::fsops::error_message(crate::fsops::last_errno());
                unsafe {
                    emit_log_parts(
                        env,
                        4,
                        &[
                            b"Error restoring diskstate: Could not rename file ",
                            &self.temp,
                            b" to ",
                            &self.dest,
                            b": ",
                            &err,
                        ],
                    );
                }
                return None;
            }
        }

        let mut reader = match RsStateReader::open(&self.dest_c) {
            Ok(r) => r,
            Err(errno) => {
                let err = crate::fsops::error_message(errno);
                unsafe {
                    emit_log_parts(
                        env,
                        4,
                        &[
                            b"Error reading diskstate: could not open file ",
                            &self.dest,
                            b": ",
                            &err,
                        ],
                    );
                }
                return None;
            }
        };

        let sig = reader.read_line(128).unwrap_or(b"");
        self.file_version = parse_format_version(sig);
        if self.file_version > self.read_version {
            unsafe {
                emit_log_parts(
                    env,
                    4,
                    &[
                        b"Could not load diskstate file ",
                        &self.dest,
                        b" due to file version mismatch",
                    ],
                );
            }
            return None;
        }

        Some(reader)
    }
}

#[derive(Default)]
struct TrailerAccum {
    dupe_count: i32,
    dupe_bytes: Vec<u8>,
    unpack_count: i32,
    unpack_bytes: Vec<u8>,
}

impl TrailerAccum {
    unsafe fn push_nzb(&mut self, exp: &NzbgetRsDiskNzbExport) {
        if has_dupe_extras(
            exp.dupe_recovered_articles,
            exp.dupe_recovered_bytes,
            exp.dupe_recovered_holes,
            exp.jobs_len,
        ) {
            self.dupe_count += 1;
            let _ = writeln!(self.dupe_bytes, "{}", exp.info.id);
            let extras = unsafe {
                format_dupe_extras_bytes(
                    exp.dupe_recovered_articles,
                    exp.dupe_recovered_bytes,
                    exp.dupe_recovered_holes,
                    exp.jobs,
                    exp.jobs_len,
                )
            };
            self.dupe_bytes.extend_from_slice(&extras);
        }
        self.unpack_count += 1;
        let unpack = unsafe {
            format_unpack_trailer_item_bytes(
                exp.info.id,
                exp.direct_unpack_status,
                exp.unpack_tried,
                exp.par_repaired,
                exp.last_unpack_status,
                exp.archives,
                exp.archives_len,
            )
        };
        self.unpack_bytes.extend_from_slice(&unpack);
    }

    fn write_to<W: Write>(&self, w: &mut W) {
        let _ = w.write_all(DUPESTATE_TRAILER);
        let _ = writeln!(w, "\n{}", self.dupe_count);
        let _ = w.write_all(&self.dupe_bytes);
        let _ = w.write_all(UNPACKSTATE_TRAILER);
        let _ = writeln!(w, "\n{}", self.unpack_count);
        let _ = w.write_all(&self.unpack_bytes);
    }
}

unsafe fn save_nzb_section(
    env: &NzbgetRsDiskStateEnv,
    w: &mut std::io::BufWriter<std::fs::File>,
    source: c_int,
    header_count: i32,
) {
    let Some(export_cb) = env.export_nzb_cb else {
        return;
    };
    let iter_len = if source == 1 {
        unsafe { call_ctrl(env, 1, 0, 0, 0, std::ptr::null()) as usize }
    } else {
        unsafe { call_ctrl(env, 0, 0, 0, 0, std::ptr::null()) as usize }
    };
    let _ = writeln!(w, "{header_count}");

    struct SinkState<'a> {
        w: &'a mut std::io::BufWriter<std::fs::File>,
        source: c_int,
        remaining_progress: i32,
        trailer: TrailerAccum,
    }
    unsafe extern "C" fn sink_cb(sink_ctx: *mut c_void, exp_ptr: *const NzbgetRsDiskNzbExport) {
        if exp_ptr.is_null() {
            return;
        }
        let st = unsafe { &mut *sink_ctx.cast::<SinkState<'_>>() };
        let exp = unsafe { &*exp_ptr };
        if st.source == 0 {
            let bytes = unsafe {
                format_nzb_info_bytes(
                    &exp.info,
                    exp.completed,
                    exp.completed_len,
                    exp.params,
                    exp.params_len,
                    exp.scripts,
                    exp.scripts_len,
                    exp.stats,
                    exp.stats_len,
                    exp.files,
                    exp.files_len,
                )
            };
            let _ = st.w.write_all(&bytes);
            unsafe { st.trailer.push_nzb(exp) };
        } else if st.source == 2 {
            if exp.changed != 0 && st.remaining_progress > 0 {
                st.remaining_progress -= 1;
                let _ = writeln!(st.w, "{}", exp.info.id);
                let bytes = unsafe {
                    format_nzb_info_bytes(
                        &exp.info,
                        exp.completed,
                        exp.completed_len,
                        exp.params,
                        exp.params_len,
                        exp.scripts,
                        exp.scripts_len,
                        exp.stats,
                        exp.stats_len,
                        exp.files,
                        exp.files_len,
                    )
                };
                let _ = st.w.write_all(&bytes);
                unsafe { st.trailer.push_nzb(exp) };
            }
        } else if st.source == 1 {
            let _ = writeln!(st.w, "{},{},{}", exp.hist_id, exp.hist_kind, exp.hist_time);
            if exp.hist_kind == 1 || exp.hist_kind == 2 {
                let bytes = unsafe {
                    format_nzb_info_bytes(
                        &exp.info,
                        exp.completed,
                        exp.completed_len,
                        exp.params,
                        exp.params_len,
                        exp.scripts,
                        exp.scripts_len,
                        exp.stats,
                        exp.stats_len,
                        exp.files,
                        exp.files_len,
                    )
                };
                let _ = st.w.write_all(&bytes);
                unsafe { st.trailer.push_nzb(exp) };
            } else if exp.hist_kind == 3 {
                let bytes = unsafe {
                    format_dup_info_bytes(
                        exp.info.par_status,
                        exp.info.size,
                        exp.info.full_content_hash,
                        exp.info.filtered_content_hash,
                        exp.info.dupe_score,
                        exp.info.dupe_mode,
                        exp.info.name,
                        exp.info.dupe_key,
                    )
                };
                let _ = st.w.write_all(&bytes);
            }
        }
    }

    let mut state = SinkState {
        w,
        source,
        remaining_progress: header_count,
        trailer: TrailerAccum::default(),
    };
    for idx in 0..iter_len {
        unsafe {
            export_cb(
                env.ctx,
                source,
                idx,
                (&mut state as *mut SinkState<'_>).cast(),
                sink_cb,
            );
        }
    }
    state.trailer.write_to(state.w);
}

unsafe fn empty_nzb_input() -> NzbgetRsDiskNzbInfoInput {
    unsafe { std::mem::zeroed() }
}

unsafe fn load_dupe_extras_into_target(
    env: &NzbgetRsDiskStateEnv,
    r: &mut RsStateReader,
    format_version: i32,
) -> bool {
    unsafe extern "C" fn on_hdr(
        ctx: *mut c_void,
        rec_articles: c_int,
        rec_bytes: i64,
        rec_holes: c_int,
    ) {
        let env = unsafe { &*ctx.cast::<NzbgetRsDiskStateEnv>() };
        if let Some(cb) = env.apply_nzb_cb {
            let mut inp = unsafe { empty_nzb_input() };
            inp.total_articles = rec_articles;
            inp.size = rec_bytes;
            inp.failed_articles = rec_holes;
            unsafe {
                cb(
                    env.ctx,
                    1,
                    &inp,
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                );
            }
        }
    }
    unsafe extern "C" fn on_job(ctx: *mut c_void, job: *const NzbgetRsDiskRepairJob) {
        let env = unsafe { &*ctx.cast::<NzbgetRsDiskStateEnv>() };
        if let Some(cb) = env.apply_nzb_cb {
            let inp = unsafe { empty_nzb_input() };
            unsafe {
                cb(
                    env.ctx,
                    2,
                    &inp,
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    job,
                );
            }
        }
    }
    unsafe {
        load_dupe_extras(
            r.as_ctx(),
            RsStateReader::read_line_cb,
            format_version,
            (env as *const NzbgetRsDiskStateEnv).cast_mut().cast(),
            on_hdr,
            on_job,
        )
    }
}

unsafe fn parse_unpack_state_into_target(
    env: &NzbgetRsDiskStateEnv,
    r: &mut RsStateReader,
) -> bool {
    let Some(l1) = r.read_line(1024).map(|b| b.to_vec()) else {
        return false;
    };
    let Some(l2) = r.read_line(1024).map(|b| b.to_vec()) else {
        return false;
    };
    let Some(l3) = r.read_line(1024).map(|b| b.to_vec()) else {
        return false;
    };
    let Some((direct, tried, repaired, last)) =
        parse_unpack_state_lines(&l1, &l2, &l3, env.ns_running, env.ns_none)
    else {
        return false;
    };
    let Some(l4) = r.read_line(65536).map(|b| b.to_vec()) else {
        return false;
    };
    if let Some(cb) = env.apply_nzb_cb {
        let mut inp = unsafe { empty_nzb_input() };
        inp.par_status = direct;
        inp.unpack_status = c_int::from(tried);
        inp.move_status = c_int::from(repaired);
        inp.delete_status = last;
        unsafe {
            cb(
                env.ctx,
                3,
                &inp,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
            );
        }
        let archives = l4.strip_prefix(b"ExtractedArchives=").unwrap_or(&l4);
        let mut cursor = 0;
        while let Some((start, len, next)) = diskstate_next_comma_token(archives, cursor) {
            let mut tok_inp = unsafe { empty_nzb_input() };
            tok_inp.name = unsafe { archives.as_ptr().add(start).cast() };
            unsafe {
                cb(
                    env.ctx,
                    4,
                    &tok_inp,
                    len as c_int,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                );
            }
            cursor = next;
        }
    }
    true
}

unsafe fn load_nzb_info_into_target(
    env: &NzbgetRsDiskStateEnv,
    r: &mut RsStateReader,
    format_version: i32,
) -> bool {
    unsafe extern "C" fn on_loaded(
        ctx: *mut c_void,
        inp: *const NzbgetRsDiskNzbInfoInput,
        flags: c_int,
        completed: *const NzbgetRsDiskCompletedFile,
        completed_len: usize,
        params: *const NzbgetRsDiskParam,
        params_len: usize,
        scripts: *const NzbgetRsDiskScriptStatus,
        scripts_len: usize,
        stats: *const NzbgetRsDiskServerStat,
        stats_len: usize,
        files: *const NzbgetRsDiskFileStub,
        file_times: *const c_int,
        files_len: usize,
    ) {
        let env = unsafe { &*ctx.cast::<NzbgetRsDiskStateEnv>() };
        if let Some(cb) = env.apply_nzb_cb {
            unsafe {
                cb(
                    env.ctx,
                    0,
                    inp,
                    flags,
                    completed,
                    completed_len,
                    params,
                    params_len,
                    scripts,
                    scripts_len,
                    stats,
                    stats_len,
                    files,
                    file_times,
                    files_len,
                    std::ptr::null(),
                );
            }
        }
    }

    let ok = unsafe {
        load_nzb_info(
            r.as_ctx(),
            RsStateReader::read_line_cb,
            format_version,
            env.pt_finished,
            (env as *const NzbgetRsDiskStateEnv).cast_mut().cast(),
            on_loaded,
        )
    } && (!matches!(format_version, 65 | 66)
        || unsafe { load_dupe_extras_into_target(env, r, format_version) })
        && (format_version < 67 || unsafe { parse_unpack_state_into_target(env, r) });

    if !ok {
        unsafe { emit_log(env, 4, b"Error reading nzb info from disk") };
    }
    ok
}

unsafe fn load_dupe_trailer_section(
    env: &NzbgetRsDiskStateEnv,
    r: &mut RsStateReader,
    is_history: bool,
) -> bool {
    let Some(first) = r.read_line(1024).map(|b| b.to_vec()) else {
        return true;
    };
    if first != DUPESTATE_TRAILER {
        return true;
    }
    let ok = (|| -> bool {
        let Some(count) = r.scan_i32() else {
            return false;
        };
        if count < 0 {
            return false;
        }
        let target_src = c_int::from(is_history);
        for _ in 0..count {
            let Some(id) = r.scan_i32() else {
                return false;
            };
            unsafe { call_ctrl(env, 13, target_src, id, 0, std::ptr::null()) };
            if !unsafe { load_dupe_extras_into_target(env, r, DISKSTATE_QUEUE_LEGACY_VERSION) } {
                return false;
            }
        }
        if let Some(next_line) = r.read_line(1024).map(|b| b.to_vec()) {
            if next_line == UNPACKSTATE_TRAILER {
                let Some(unpack_count) = r.scan_i32() else {
                    return false;
                };
                if unpack_count < 0 {
                    return false;
                }
                for _ in 0..unpack_count {
                    let Some(id) = r.scan_i32() else {
                        return false;
                    };
                    unsafe { call_ctrl(env, 13, target_src, id, 0, std::ptr::null()) };
                    if !unsafe { parse_unpack_state_into_target(env, r) } {
                        return false;
                    }
                }
            }
        }
        true
    })();
    if !ok {
        unsafe { emit_log(env, 4, b"Error reading duplicate repair state from disk") };
    }
    ok
}

unsafe fn load_queue_section(
    env: &NzbgetRsDiskStateEnv,
    r: &mut RsStateReader,
    format_version: i32,
) -> bool {
    unsafe { emit_log(env, 0, b"Loading nzb list from disk") };
    let ok = (|| -> bool {
        let Some(size) = r.scan_i32() else {
            return false;
        };
        for _ in 0..size {
            unsafe { call_ctrl(env, 9, 0, 0, 0, std::ptr::null()) };
            if !unsafe { load_nzb_info_into_target(env, r, format_version) } {
                return false;
            }
            unsafe { call_ctrl(env, 10, 0, 0, 0, std::ptr::null()) };
        }
        if (57..=DISKSTATE_QUEUE_VERSION).contains(&format_version)
            && !unsafe { load_dupe_trailer_section(env, r, false) }
        {
            return false;
        }
        true
    })();
    if !ok {
        unsafe { emit_log(env, 4, b"Error reading nzb list from disk") };
    }
    ok
}

unsafe fn load_progress_section(
    env: &NzbgetRsDiskStateEnv,
    r: &mut RsStateReader,
    format_version: i32,
) -> bool {
    unsafe { emit_log(env, 0, b"Loading nzb progress from disk") };
    let ok = (|| -> bool {
        let Some(size) = r.scan_i32() else {
            return false;
        };
        for _ in 0..size {
            let Some(id) = r.scan_i32() else {
                return false;
            };
            let found = unsafe { call_ctrl(env, 11, id, 0, 0, std::ptr::null()) != 0 };
            if !found {
                unsafe {
                    emit_log(
                        env,
                        3,
                        format!(
                            "Skipping saved progress of NZB with id {id}, which is no longer queued"
                        )
                        .as_bytes(),
                    );
                }
            }
            if !unsafe { load_nzb_info_into_target(env, r, format_version) } {
                return false;
            }
        }
        if format_version <= DISKSTATE_QUEUE_VERSION
            && !unsafe { load_dupe_trailer_section(env, r, false) }
        {
            return false;
        }
        true
    })();
    if !ok {
        unsafe { emit_log(env, 4, b"Error reading nzb progress from disk") };
    }
    ok
}

unsafe fn load_history_section(
    env: &NzbgetRsDiskStateEnv,
    r: &mut RsStateReader,
    format_version: i32,
) -> bool {
    unsafe { emit_log(env, 0, b"Loading history from disk") };
    let ok = (|| -> bool {
        let Some(size) = r.scan_i32() else {
            return false;
        };
        for _ in 0..size {
            let Some(hdr) = r.read_line(1024).map(|b| b.to_vec()) else {
                return false;
            };
            let mut sc = ScanCursor(&hdr);
            let (Some(id), Some(kindval), Some(time)) = (sc.i32(false), sc.i32(true), sc.i32(true))
            else {
                return false;
            };
            if kindval == 1 || kindval == 2 {
                unsafe { call_ctrl(env, 9, 0, 0, 0, std::ptr::null()) };
                if !unsafe { load_nzb_info_into_target(env, r, format_version) } {
                    return false;
                }
                unsafe { call_ctrl(env, 12, kindval, time, 0, std::ptr::null()) };
            } else if kindval == 3 {
                struct DupCtx<'a> {
                    env: &'a NzbgetRsDiskStateEnv,
                    id: c_int,
                    time: c_int,
                }
                unsafe extern "C" fn on_dup(
                    ctx: *mut c_void,
                    status: c_int,
                    dupe_score: c_int,
                    dupe_mode: c_int,
                    full_hash: u32,
                    filt_hash: u32,
                    size: i64,
                    name: *const c_char,
                    dupe_key: *const c_char,
                ) {
                    let dc = unsafe { &*ctx.cast::<DupCtx<'_>>() };
                    if let Some(cb) = dc.env.apply_nzb_cb {
                        let mut inp = unsafe { empty_nzb_input() };
                        inp.id = dc.id;
                        inp.feed_id = dc.time;
                        inp.par_status = status;
                        inp.size = size;
                        inp.full_content_hash = full_hash;
                        inp.filtered_content_hash = filt_hash;
                        inp.dupe_score = dupe_score;
                        inp.dupe_mode = dupe_mode;
                        inp.name = name;
                        inp.dupe_key = dupe_key;
                        unsafe {
                            cb(
                                dc.env.ctx,
                                5,
                                &inp,
                                0,
                                std::ptr::null(),
                                0,
                                std::ptr::null(),
                                0,
                                std::ptr::null(),
                                0,
                                std::ptr::null(),
                                0,
                                std::ptr::null(),
                                std::ptr::null(),
                                0,
                                std::ptr::null(),
                            );
                        }
                    }
                }
                let mut dc = DupCtx { env, id, time };
                if !unsafe {
                    load_section(
                        0,
                        r.as_ctx(),
                        Some(RsStateReader::read_line_cb),
                        format_version,
                        &[],
                        0,
                        0,
                        (&mut dc as *mut DupCtx<'_>).cast(),
                        on_dup,
                    )
                } {
                    return false;
                }
            } else {
                unsafe {
                    emit_log(
                        env,
                        4,
                        format!("Unknown history record kind {kindval}").as_bytes(),
                    );
                }
                return false;
            }
        }
        if (57..=DISKSTATE_QUEUE_VERSION).contains(&format_version)
            && !unsafe { load_dupe_trailer_section(env, r, true) }
        {
            return false;
        }
        true
    })();
    if !ok {
        unsafe { emit_log(env, 4, b"Error reading diskstate for history") };
    }
    ok
}

struct QueriedFileMeta {
    file_id: i32,
    has_articles: i32,
    articles_len: i32,
    force_direct_write: i32,
    output_initialized: i32,
    dest_dir: Vec<u8>,
    filename: Vec<u8>,
    output_filename: Vec<u8>,
}

unsafe fn query_target_file_meta(env: &NzbgetRsDiskStateEnv) -> QueriedFileMeta {
    let mut meta = QueriedFileMeta {
        file_id: 0,
        has_articles: 0,
        articles_len: 0,
        force_direct_write: 0,
        output_initialized: 0,
        dest_dir: Vec::new(),
        filename: Vec::new(),
        output_filename: Vec::new(),
    };
    unsafe extern "C" fn sink(sink_ctx: *mut c_void, exp_ptr: *const NzbgetRsDiskFileExport) {
        if exp_ptr.is_null() {
            return;
        }
        let m = unsafe { &mut *sink_ctx.cast::<QueriedFileMeta>() };
        let exp = unsafe { &*exp_ptr };
        m.file_id = exp.file_id;
        m.has_articles = exp.has_articles;
        m.articles_len = exp.articles_len_count;
        m.force_direct_write = exp.force_direct_write;
        m.output_initialized = exp.info.output_initialized;
        m.dest_dir = unsafe { cstr_to_bytes(exp.info.subject) }.to_vec();
        m.filename = unsafe { cstr_to_bytes(exp.info.filename) }.to_vec();
        m.output_filename = unsafe { cstr_to_bytes(exp.info.output_filename) }.to_vec();
    }
    if let Some(cb) = env.export_file_cb {
        unsafe { cb(env.ctx, 4, (&mut meta as *mut QueriedFileMeta).cast(), sink) };
    }
    meta
}

unsafe fn format_target_file_bytes(env: &NzbgetRsDiskStateEnv, mode: c_int) -> (i32, Vec<u8>) {
    struct Out {
        file_id: i32,
        mode: c_int,
        bytes: Vec<u8>,
    }
    unsafe extern "C" fn sink(sink_ctx: *mut c_void, exp_ptr: *const NzbgetRsDiskFileExport) {
        if exp_ptr.is_null() {
            return;
        }
        let o = unsafe { &mut *sink_ctx.cast::<Out>() };
        let exp = unsafe { &*exp_ptr };
        o.file_id = exp.file_id;
        if o.mode == 0 || o.mode == 1 {
            let incl = c_int::from(o.mode == 0);
            o.bytes = unsafe {
                format_file_info_bytes(
                    &exp.info,
                    exp.groups,
                    exp.groups_len,
                    incl,
                    exp.articles,
                    exp.articles_len,
                )
            };
        } else {
            o.bytes = unsafe {
                format_file_state_bytes(
                    &exp.state,
                    exp.stats,
                    exp.stats_len,
                    exp.article_states,
                    exp.article_states_len,
                )
            };
        }
    }
    let mut out = Out {
        file_id: 0,
        mode,
        bytes: Vec::new(),
    };
    if let Some(cb) = env.export_file_cb {
        unsafe { cb(env.ctx, mode, (&mut out as *mut Out).cast(), sink) };
    }
    (out.file_id, out.bytes)
}

unsafe fn load_file_info_from_reader(
    env: &NzbgetRsDiskStateEnv,
    r: &mut RsStateReader,
    format_version: i32,
    file_summary: bool,
    articles: bool,
) -> bool {
    let meta = unsafe { query_target_file_meta(env) };
    struct Ctx<'a> {
        env: &'a NzbgetRsDiskStateEnv,
        summary: c_int,
    }
    unsafe extern "C" fn on_sum(
        ctx: *mut c_void,
        fields: *const NzbgetRsDiskFileInfoInput,
        flags: c_int,
    ) {
        let c = unsafe { &*ctx.cast::<Ctx<'_>>() };
        if let Some(cb) = c.env.apply_file_cb {
            unsafe {
                cb(
                    c.env.ctx,
                    0,
                    flags,
                    c.summary,
                    0,
                    0,
                    std::ptr::null(),
                    fields,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                );
            }
        }
    }
    unsafe extern "C" fn on_item(
        ctx: *mut c_void,
        grp: *const c_char,
        art: *const NzbgetRsDiskArticleInfo,
    ) {
        let c = unsafe { &*ctx.cast::<Ctx<'_>>() };
        if let Some(cb) = c.env.apply_file_cb {
            unsafe {
                cb(
                    c.env.ctx,
                    1,
                    0,
                    0,
                    0,
                    0,
                    grp,
                    std::ptr::null(),
                    art,
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                );
            }
        }
    }
    let mut fctx = Ctx {
        env,
        summary: c_int::from(file_summary),
    };
    let ok = unsafe {
        load_file_info(
            r.as_ctx(),
            RsStateReader::read_line_cb,
            format_version,
            c_int::from(file_summary),
            c_int::from(articles),
            meta.file_id,
            env.direct_write,
            meta.force_direct_write,
            env.sep as u8,
            &meta.dest_dir,
            &meta.filename,
            &meta.output_filename,
            (&mut fctx as *mut Ctx<'_>).cast(),
            on_sum,
            on_item,
        )
    };
    if !ok {
        unsafe {
            emit_log(
                env,
                4,
                format!("Error reading diskstate for file {}", meta.file_id).as_bytes(),
            );
        }
    }
    ok
}

unsafe fn load_file_state_from_reader(
    env: &NzbgetRsDiskStateEnv,
    r: &mut RsStateReader,
    format_version: i32,
    completed: bool,
    temp_dir: &[u8],
) -> bool {
    let meta = unsafe { query_target_file_meta(env) };
    struct Ctx<'a> {
        env: &'a NzbgetRsDiskStateEnv,
        ver: i32,
        has_art: i32,
    }
    unsafe extern "C" fn art_cb(
        ctx: *mut c_void,
        op: c_int,
        idx: c_int,
        status: c_int,
        seg_offset: i64,
        seg_size: c_int,
        crc: u32,
        fb_round: c_int,
        res_fn: *const c_char,
    ) -> c_int {
        let c = unsafe { &*ctx.cast::<Ctx<'_>>() };
        let Some(cb) = c.env.apply_file_cb else {
            return 0;
        };
        if op == 0 {
            unsafe {
                cb(
                    c.env.ctx,
                    2,
                    idx,
                    c.has_art,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                )
            }
        } else {
            let st = NzbgetRsDiskFileStateInput {
                success_articles: seg_size,
                failed_articles: fb_round,
                remaining_size: 0,
                success_size: 0,
                failed_size: 0,
                filename: std::ptr::null(),
                hash16k: std::ptr::null(),
                par_set_id: std::ptr::null(),
                par_file: status,
                decoded_file_size: 0,
                output_initialized: 0,
            };
            unsafe {
                cb(
                    c.env.ctx,
                    3,
                    idx,
                    c.ver,
                    seg_offset,
                    crc,
                    res_fn,
                    std::ptr::null(),
                    std::ptr::null(),
                    &st,
                    std::ptr::null(),
                    0,
                )
            }
        }
    }
    unsafe extern "C" fn sum_cb(
        ctx: *mut c_void,
        fields: *const NzbgetRsDiskFileStateInput,
        flags: c_int,
        completed_articles: c_int,
        new_out_fn: *const c_char,
        stats: *const NzbgetRsDiskServerStat,
        stats_len: usize,
    ) {
        let c = unsafe { &*ctx.cast::<Ctx<'_>>() };
        if let Some(cb) = c.env.apply_file_cb {
            unsafe {
                cb(
                    c.env.ctx,
                    4,
                    flags,
                    completed_articles,
                    0,
                    0,
                    new_out_fn,
                    std::ptr::null(),
                    std::ptr::null(),
                    fields,
                    stats,
                    stats_len,
                );
            }
        }
    }
    let mut sctx = Ctx {
        env,
        ver: format_version,
        has_art: meta.has_articles,
    };
    let ok = unsafe {
        load_file_state(
            r.as_ctx(),
            RsStateReader::read_line_cb,
            format_version,
            c_int::from(completed),
            meta.file_id,
            meta.has_articles,
            meta.articles_len,
            env.has_options,
            env.direct_write,
            meta.force_direct_write,
            meta.output_initialized,
            env.sep as u8,
            temp_dir,
            &meta.dest_dir,
            &meta.filename,
            &meta.output_filename,
            (&mut sctx as *mut Ctx<'_>).cast(),
            art_cb,
            sum_cb,
        )
    };
    if !ok {
        unsafe {
            emit_log(
                env,
                4,
                format!("Error reading diskstate for file {}", meta.file_id).as_bytes(),
            );
        }
    }
    ok
}

unsafe fn save_single_file_or_state(
    env: &NzbgetRsDiskStateEnv,
    qdir: &[u8],
    sep: u8,
    mode: c_int,
    replace: bool,
) -> bool {
    let export_mode = if mode == 0 {
        0
    } else if mode == 2 {
        3
    } else {
        2
    };
    let (file_id, bytes) = unsafe { format_target_file_bytes(env, export_mode) };
    if mode == 0 {
        unsafe {
            emit_log(
                env,
                0,
                format!("Saving FileInfo {file_id} to disk").as_bytes(),
            );
        }
    } else {
        unsafe {
            emit_log(
                env,
                0,
                format!("Saving FileState {file_id} to disk").as_bytes(),
            );
        }
    }
    let fname = if mode == 0 {
        format!("{file_id}").into_bytes()
    } else {
        diskstate_format_file_state_name(file_id, mode == 2)
    };
    let mut sf = RsStateFile::new(
        qdir,
        sep,
        &fname,
        DISKSTATE_FILE_VERSION,
        replace,
        DISKSTATE_FILE_VERSION,
    );
    sf.flush = !replace;
    let Some(mut w) = (unsafe { sf.begin_write(env) }) else {
        return false;
    };
    let _ = w.write_all(&bytes);
    unsafe { sf.finish_write(env, w, env.flush_queue != 0) }
}

unsafe fn load_single_file_or_state(
    env: &NzbgetRsDiskStateEnv,
    qdir: &[u8],
    temp_dir: &[u8],
    sep: u8,
    mode: c_int,
    file_summary: bool,
    articles: bool,
    out_ver: &mut i32,
) -> bool {
    let meta = unsafe { query_target_file_meta(env) };
    unsafe {
        emit_log(
            env,
            0,
            format!("Loading FileInfo {} from disk", meta.file_id).as_bytes(),
        );
    }
    let fname = if mode == 0 {
        format!("{}", meta.file_id).into_bytes()
    } else {
        diskstate_format_file_state_name(meta.file_id, mode == 2)
    };
    let mut sf = RsStateFile::new(
        qdir,
        sep,
        &fname,
        DISKSTATE_FILE_VERSION,
        false,
        DISKSTATE_FILE_LEGACY_VERSION,
    );
    let Some(mut r) = (unsafe { sf.begin_read(env) }) else {
        return false;
    };
    *out_ver = sf.file_version;
    if mode == 0 {
        unsafe { load_file_info_from_reader(env, &mut r, sf.file_version, file_summary, articles) }
    } else {
        unsafe {
            load_file_state_from_reader(env, &mut r, sf.file_version, mode == 2, temp_dir)
        }
    }
}

unsafe fn load_all_file_infos_op(env: &NzbgetRsDiskStateEnv, qdir: &[u8], sep: u8) {
    let nzb_count = unsafe { call_ctrl(env, 0, 0, 0, 0, std::ptr::null()) };
    if nzb_count <= 0 {
        return;
    }

    let mut sf = RsStateFile::new(
        qdir,
        sep,
        b"files",
        DISKSTATE_FILE_VERSION,
        false,
        DISKSTATE_FILE_LEGACY_VERSION,
    );
    let mut hib_reader: Option<RsStateReader> = None;
    let mut hib_ver = 0;
    if sf.file_exists() {
        if let Some(mut r) = unsafe { sf.begin_read(env) } {
            let total_files = unsafe { call_ctrl(env, 4, 0, 0, 0, std::ptr::null()) };
            if r.scan_i32() == Some(total_files) {
                hib_ver = sf.file_version;
                hib_reader = Some(r);
            }
        }
        if hib_reader.is_none() {
            sf.discard();
        }
    }

    for nzb_idx in 0..nzb_count {
        let fl_len = unsafe { call_ctrl(env, 5, nzb_idx, 0, 0, std::ptr::null()) };
        let mut broken_ids = Vec::new();
        for file_idx in 0..fl_len {
            let file_id = unsafe { call_ctrl(env, 6, nzb_idx, file_idx, 0, std::ptr::null()) };
            let mut res = false;
            if let Some(ref mut r) = hib_reader {
                let scanned_id = r.scan_i32().unwrap_or(0);
                if scanned_id == file_id {
                    res = unsafe { load_file_info_from_reader(env, r, hib_ver, true, false) };
                }
            }
            if !res {
                let mut dummy_ver = 0;
                res = unsafe {
                    load_single_file_or_state(env, qdir, &[], sep, 0, true, false, &mut dummy_ver)
                };
            }
            if !res {
                broken_ids.push(file_id);
            }
        }
        for bad_id in broken_ids {
            unsafe { call_ctrl(env, 7, nzb_idx, bad_id, 0, std::ptr::null()) };
        }
    }
}

unsafe fn cleanup_queue_dir_op(env: &NzbgetRsDiskStateEnv, qdir: &[u8], sep: u8) {
    struct IdLists {
        nzb_ids: Vec<i32>,
        file_ids: Vec<i32>,
    }
    unsafe extern "C" fn collect_sink(
        sink_ctx: *mut c_void,
        kind: c_int,
        id: c_int,
        _a2: c_int,
        _u1: u32,
        _s1: *const c_char,
        _srv: *const NzbgetRsServerMatchItem,
        _srv_len: usize,
        _vol: *const NzbgetRsDiskVolumeStatInput,
        _vol_len: usize,
    ) {
        let lists = unsafe { &mut *sink_ctx.cast::<IdLists>() };
        if kind != 0 {
            lists.nzb_ids.push(id);
        } else {
            lists.file_ids.push(id);
        }
    }
    let mut lists = IdLists {
        nzb_ids: Vec::new(),
        file_ids: Vec::new(),
    };
    if let Some(aux) = env.aux_cb {
        unsafe {
            aux(
                env.ctx,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                (&mut lists as *mut IdLists).cast(),
                Some(collect_sink),
            );
        }
    }
    lists.nzb_ids.sort_unstable();
    lists.file_ids.sort_unstable();

    let mut deleted_files = 0i32;
    for filename in crate::fsops::list_dir_entries(qdir) {
        if is_orphan_queue_file(&filename, &lists.file_ids, &lists.nzb_ids) {
            unsafe {
                emit_log_parts(
                    env,
                    1,
                    &[b"Deleting orphaned diskstate file ", &filename],
                );
            }
            let p =
                crate::artwritercore::article_writer_format_dest_file_path(qdir, sep, &filename);
            crate::fsops::delete_file(&bytes_to_cstring_trunc(&p));
            deleted_files += 1;
        }
    }
    if deleted_files > 0 {
        unsafe {
            emit_log(
                env,
                2,
                format!("Deleted {deleted_files} orphaned diskstate file(s)").as_bytes(),
            );
        }
    }
}

unsafe fn load_all_file_states_op(
    env: &NzbgetRsDiskStateEnv,
    qdir: &[u8],
    temp_dir: &[u8],
    sep: u8,
) -> bool {
    let mut o1 = 0;
    let mut o2 = 0;
    let cache_was_active = diskstate_fs_op(6, qdir, sep, 0, 0, 0, 0, &mut o1, &mut o2) != 0;

    for filename in crate::fsops::list_dir_entries(qdir) {
        if let Some((id, suffix)) = diskstate_parse_queue_file_id_and_suffix(&filename) {
            if suffix == b'c' || (env.continue_partial != 0 && !cache_was_active) {
                let found = unsafe { call_ctrl(env, 14, id, 0, 0, std::ptr::null()) != 0 };
                if found {
                    let completed = suffix == b'c';
                    let mode = if completed { 2 } else { 1 };
                    let mut state_ver = 0;
                    if !unsafe {
                        load_single_file_or_state(
                            env,
                            qdir,
                            temp_dir,
                            sep,
                            mode,
                            false,
                            false,
                            &mut state_ver,
                        )
                    } {
                        unsafe {
                            emit_log_parts(
                                env,
                                3,
                                &[b"Discarding damaged download state ", &filename],
                            );
                            call_ctrl(env, 15, 0, 0, 0, std::ptr::null());
                        }
                        let p = crate::artwritercore::article_writer_format_dest_file_path(
                            qdir, sep, &filename,
                        );
                        crate::fsops::delete_file(&bytes_to_cstring_trunc(&p));
                        continue;
                    }
                    if state_ver > DISKSTATE_FILE_VERSION {
                        let _ = unsafe { save_single_file_or_state(env, qdir, sep, mode, false) };
                    }
                    unsafe {
                        call_ctrl(env, 16, c_int::from(completed), 0, 0, std::ptr::null());
                    }
                }
            } else {
                let p = crate::artwritercore::article_writer_format_dest_file_path(
                    qdir, sep, &filename,
                );
                crate::fsops::delete_file(&bytes_to_cstring_trunc(&p));
            }
        }
    }
    true
}

unsafe fn convert_legacy_file_states_op(
    env: &NzbgetRsDiskStateEnv,
    qdir: &[u8],
    temp_dir: &[u8],
    sep: u8,
) {
    let mut legacy: Vec<(i32, i32)> = Vec::new();
    unsafe extern "C" fn push_legacy(
        ctx: *mut c_void,
        id: c_int,
        suffix_kind: c_int,
        _a3: c_int,
        _u1: u32,
        _u2: u32,
        _i64_1: i64,
        _s1: *const c_char,
        _s2: *const c_char,
    ) {
        unsafe { (*ctx.cast::<Vec<(i32, i32)>>()).push((id, suffix_kind)) };
    }
    unsafe {
        load_section(
            4,
            std::ptr::null_mut(),
            None,
            DISKSTATE_FILE_VERSION,
            qdir,
            sep,
            i64::from(DISKSTATE_FILE_LEGACY_VERSION),
            (&mut legacy as *mut Vec<(i32, i32)>).cast(),
            push_legacy,
        );
    }
    if legacy.is_empty() {
        return;
    }

    let mut converted = 0i32;
    for &(id, suffix_kind) in &legacy {
        unsafe { call_ctrl(env, 17, id, 0, 0, std::ptr::null()) };
        let mut dummy_ver = 0;
        let ok = if suffix_kind == 0 {
            unsafe {
                load_single_file_or_state(env, qdir, temp_dir, sep, 0, true, true, &mut dummy_ver)
                    && save_single_file_or_state(env, qdir, sep, 0, true)
            }
        } else {
            let mode = if suffix_kind == 2 { 2 } else { 1 };
            unsafe {
                load_single_file_or_state(env, qdir, temp_dir, sep, 0, false, true, &mut dummy_ver)
                    && load_single_file_or_state(
                        env,
                        qdir,
                        temp_dir,
                        sep,
                        mode,
                        false,
                        false,
                        &mut dummy_ver,
                    )
                    && save_single_file_or_state(env, qdir, sep, mode, true)
            }
        };
        if ok {
            converted += 1;
        }
    }
    unsafe {
        emit_log(
            env,
            2,
            format!(
                "Converted {} of {} file state(s) of an earlier build to the upstream format",
                converted,
                legacy.len()
            )
            .as_bytes(),
        );
    }
}

unsafe fn set_aside_queue_files_op(env: &NzbgetRsDiskStateEnv, qdir: &[u8], sep: u8) {
    unsafe extern "C" fn log_kept(
        ctx: *mut c_void,
        _a1: c_int,
        _a2: c_int,
        _a3: c_int,
        _u1: u32,
        _u2: u32,
        _i64_1: i64,
        name: *const c_char,
        kept_base: *const c_char,
    ) {
        let env = unsafe { &*ctx.cast::<NzbgetRsDiskStateEnv>() };
        unsafe {
            emit_log_parts(
                env,
                4,
                &[
                    b"Diskstate file ",
                    cstr_to_bytes(name),
                    b" could not be read; it is kept as ",
                    cstr_to_bytes(kept_base),
                ],
            );
        }
    }
    unsafe {
        load_section(
            5,
            std::ptr::null_mut(),
            None,
            0,
            qdir,
            sep,
            env.now,
            (env as *const NzbgetRsDiskStateEnv).cast_mut().cast(),
            log_kept,
        );
    }
}

/// Unified entry point for all `DiskState` operations:
/// - `0`: `DiscardDownloadQueue`
/// - `1`: `DownloadQueueExists`
/// - `2`: `DiscardFile`
/// - `3`: `DeleteCacheFlag`
/// - `4`: `WriteCacheFlag`
/// - `5`: `DeleteQueueNzbLog` (`DiscardFiles` log deletion)
/// - `6`: `AppendNzbMessage`
/// - `7`: `LoadNzbMessages`
/// - `8`: `CleanupTempDir`
/// - `9`: `SaveFeeds`
/// - `10`: `LoadFeeds`
/// - `11`: `SaveStats`
/// - `12`: `LoadStats`
/// - `13`: `SaveFile` (`a1=0`) / `SaveFileState` (`a1=1` partial, `a1=2` completed; `a2=replace`)
/// - `14`: `LoadFile` (`a1=0`, `a2=summary`, `a3=articles`) / `LoadFileState` (`a1=1` partial, `a1=2` completed)
/// - `15`: `SaveDownloadQueue` (`a1=0, a2=saveHistory`) / `SaveDownloadProgress` (`a1=1`) / `SaveAllFileInfos` (`a1=2`)
/// - `16`: `LoadDownloadQueue`
pub unsafe fn diskstate_exec(op: c_int, env: &NzbgetRsDiskStateEnv) -> c_int {
    let qdir = unsafe { cstr_to_bytes(env.queue_dir) };
    let tdir = unsafe { cstr_to_bytes(env.temp_dir) };
    let sep = env.sep as u8;
    let mut o1 = 0;
    let mut o2 = 0;

    match op {
        0 => {
            unsafe { emit_log(env, 0, b"Discarding queue") };
            diskstate_fs_op(0, qdir, sep, 0, 0, 0, 0, &mut o1, &mut o2)
        }
        1 => {
            unsafe { emit_log(env, 0, b"Checking if a saved queue exists on disk") };
            diskstate_fs_op(1, qdir, sep, 0, 0, 0, 0, &mut o1, &mut o2)
        }
        2 => diskstate_fs_op(
            2, qdir, sep, env.a1, env.a2, env.a3, env.a4, &mut o1, &mut o2,
        ),
        3 => diskstate_fs_op(3, qdir, sep, 0, 0, 0, 0, &mut o1, &mut o2),
        4 => {
            let flag_p =
                crate::artwritercore::article_writer_format_dest_file_path(qdir, sep, b"acache");
            let flag_c = bytes_to_cstring_trunc(&flag_p);
            let fd = crate::fsops::open_cloexec_fd(&flag_c, 2, 0, 1, 2);
            if fd == -1 {
                unsafe {
                    emit_log_parts(
                        env,
                        4,
                        &[b"Error saving diskstate: Could not create file ", &flag_p],
                    );
                }
                0
            } else {
                unsafe { crate::fsops::sys::close(fd) };
                1
            }
        }
        5 => diskstate_fs_op(7, qdir, sep, env.a1, 0, 0, 0, &mut o1, &mut o2),
        6 => {
            let log_name = format!("n{}.log", env.a1);
            let log_p = crate::artwritercore::article_writer_format_dest_file_path(
                qdir,
                sep,
                log_name.as_bytes(),
            );
            let log_c = bytes_to_cstring_trunc(&log_p);
            let fd = crate::fsops::open_cloexec_fd(&log_c, 3, 0, 1, 2);
            if fd == -1 {
                unsafe {
                    emit_log_parts(
                        env,
                        4,
                        &[b"Error saving log: Could not create file ", &log_p],
                    );
                }
                return 0;
            }
            let line = unsafe {
                format_nzb_message_line_bytes(
                    env.time_str,
                    env.a2,
                    env.a3,
                    env.text,
                    env.line_ending,
                )
            };
            let mut file: std::fs::File = unsafe { std::os::unix::io::FromRawFd::from_raw_fd(fd) };
            let _ = file.write_all(&line);
            1
        }
        7 => {
            let log_name = format!("n{}.log", env.a1);
            let log_p = crate::artwritercore::article_writer_format_dest_file_path(
                qdir,
                sep,
                log_name.as_bytes(),
            );
            if crate::fsops::posix_stat(&log_p) != 1 {
                return 1;
            }
            let log_c = bytes_to_cstring_trunc(&log_p);
            let Ok(mut r) = RsStateReader::open(&log_c) else {
                unsafe {
                    emit_log_parts(
                        env,
                        4,
                        &[b"Error reading log: could not open file ", &log_p],
                    );
                }
                return 0;
            };
            let mut id = 0i32;
            while let Some(raw_line) = r.read_line(1024) {
                let trimmed = &raw_line[..crate::util::trim_right_line(raw_line)];
                let Some((time, kind, text_off)) = parse_log_line(trimmed) else {
                    break;
                };
                id += 1;
                let text_c = bytes_to_cstring_trunc(&trimmed[text_off..]);
                unsafe { call_ctrl(env, 19, id, kind, i64::from(time), text_c.as_ptr()) };
            }
            1
        }
        8 => {
            for filename in crate::fsops::list_dir_entries(tdir) {
                let (kind, id, _) = classify_temp_file(&filename);
                let garbage = kind == 1
                    || (kind == 2
                        && unsafe { call_ctrl(env, 8, id, 0, 0, std::ptr::null()) == 0 });
                if garbage {
                    let p = crate::artwritercore::article_writer_format_dest_file_path(
                        tdir, sep, &filename,
                    );
                    crate::fsops::delete_file(&bytes_to_cstring_trunc(&p));
                }
            }
            1
        }
        9 => {
            unsafe { emit_log(env, 0, b"Saving feeds state to disk") };
            struct FeedOut {
                urls: Vec<std::ffi::CString>,
                hashes: Vec<u32>,
                updates: Vec<c_int>,
                statuses: Vec<c_int>,
                seens: Vec<c_int>,
                hist_urls: Vec<std::ffi::CString>,
            }
            unsafe extern "C" fn feed_sink(
                sink_ctx: *mut c_void,
                kind: c_int,
                a1: c_int,
                a2: c_int,
                u1: u32,
                s1: *const c_char,
                _srv: *const NzbgetRsServerMatchItem,
                _srv_len: usize,
                _vol: *const NzbgetRsDiskVolumeStatInput,
                _vol_len: usize,
            ) {
                let fo = unsafe { &mut *sink_ctx.cast::<FeedOut>() };
                let c = bytes_to_cstring_trunc(unsafe { cstr_to_bytes(s1) });
                if kind == 0 {
                    fo.urls.push(c);
                    fo.hashes.push(u1);
                    fo.updates.push(a1);
                } else {
                    fo.statuses.push(a1);
                    fo.seens.push(a2);
                    fo.hist_urls.push(c);
                }
            }
            let mut fo = FeedOut {
                urls: Vec::new(),
                hashes: Vec::new(),
                updates: Vec::new(),
                statuses: Vec::new(),
                seens: Vec::new(),
                hist_urls: Vec::new(),
            };
            if let Some(aux) = env.aux_cb {
                unsafe {
                    aux(
                        env.ctx,
                        1,
                        0,
                        0,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        (&mut fo as *mut FeedOut).cast(),
                        Some(feed_sink),
                    );
                }
            }
            let sf = RsStateFile::new(
                qdir,
                sep,
                b"feeds",
                DISKSTATE_FEEDS_VERSION,
                true,
                DISKSTATE_FEEDS_VERSION,
            );
            if fo.urls.is_empty() && fo.hist_urls.is_empty() {
                sf.discard();
                return 1;
            }
            let Some(mut w) = (unsafe { sf.begin_write(env) }) else {
                return 0;
            };
            unsafe { emit_log(env, 0, b"Saving feed status to disk") };
            let url_ptrs: Vec<*const c_char> = fo.urls.iter().map(|c| c.as_ptr()).collect();
            let status_bytes = unsafe {
                format_feed_status_bytes(
                    url_ptrs.as_ptr(),
                    fo.hashes.as_ptr(),
                    fo.updates.as_ptr(),
                    url_ptrs.len(),
                )
            };
            let _ = w.write_all(&status_bytes);

            unsafe { emit_log(env, 0, b"Saving feed history to disk") };
            let hist_ptrs: Vec<*const c_char> = fo.hist_urls.iter().map(|c| c.as_ptr()).collect();
            let hist_bytes = unsafe {
                format_feed_history_bytes(
                    fo.statuses.as_ptr(),
                    fo.seens.as_ptr(),
                    hist_ptrs.as_ptr(),
                    hist_ptrs.len(),
                )
            };
            let _ = w.write_all(&hist_bytes);
            c_int::from(unsafe { sf.finish_write(env, w, env.flush_queue != 0) })
        }
        10 => {
            unsafe { emit_log(env, 0, b"Loading feeds state from disk") };
            let mut sf = RsStateFile::new(
                qdir,
                sep,
                b"feeds",
                DISKSTATE_FEEDS_VERSION,
                true,
                DISKSTATE_FEEDS_VERSION,
            );
            if !sf.file_exists() {
                return 1;
            }
            let Some(mut r) = (unsafe { sf.begin_read(env) }) else {
                return 0;
            };
            let ver = sf.file_version;
            unsafe extern "C" fn on_feed_status(
                ctx: *mut c_void,
                last_update: c_int,
                ver: c_int,
                _a3: c_int,
                filter_hash: u32,
                _u2: u32,
                _i64_1: i64,
                url: *const c_char,
                filter: *const c_char,
            ) {
                let env = unsafe { &*ctx.cast::<NzbgetRsDiskStateEnv>() };
                if let Some(aux) = env.aux_cb {
                    unsafe {
                        aux(
                            env.ctx,
                            2,
                            last_update,
                            ver,
                            filter_hash,
                            url,
                            filter,
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            None,
                        );
                    }
                }
            }
            unsafe extern "C" fn on_feed_hist(
                ctx: *mut c_void,
                status: c_int,
                last_seen: c_int,
                _a3: c_int,
                _u1: u32,
                _u2: u32,
                _i64_1: i64,
                url: *const c_char,
                _s2: *const c_char,
            ) {
                let env = unsafe { &*ctx.cast::<NzbgetRsDiskStateEnv>() };
                if let Some(aux) = env.aux_cb {
                    unsafe {
                        aux(
                            env.ctx,
                            3,
                            status,
                            last_seen,
                            0,
                            url,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            None,
                        );
                    }
                }
            }
            let env_ptr: *mut c_void = (env as *const NzbgetRsDiskStateEnv).cast_mut().cast();
            unsafe { emit_log(env, 0, b"Loading feed status from disk") };
            if !unsafe {
                load_section(
                    1,
                    r.as_ctx(),
                    Some(RsStateReader::read_line_cb),
                    ver,
                    &[],
                    0,
                    0,
                    env_ptr,
                    on_feed_status,
                )
            } {
                unsafe {
                    emit_log(env, 4, b"Error reading feed status from disk");
                    emit_log(env, 4, b"Error reading diskstate for feeds");
                }
                return 0;
            }
            unsafe { emit_log(env, 0, b"Loading feed history from disk") };
            if !unsafe {
                load_section(
                    2,
                    r.as_ctx(),
                    Some(RsStateReader::read_line_cb),
                    ver,
                    &[],
                    0,
                    0,
                    env_ptr,
                    on_feed_hist,
                )
            } {
                unsafe {
                    emit_log(env, 4, b"Error reading feed history from disk");
                    emit_log(env, 4, b"Error reading diskstate for feeds");
                }
                return 0;
            }
            1
        }
        11 => {
            unsafe { emit_log(env, 0, b"Saving stats to disk") };
            struct StatsSaveOut {
                has_servers: bool,
                srv_bytes: Vec<u8>,
                vol_count: usize,
                vol_bytes: Vec<u8>,
            }
            unsafe extern "C" fn stats_save_sink(
                sink_ctx: *mut c_void,
                _kind: c_int,
                _a1: c_int,
                _a2: c_int,
                _u1: u32,
                _s1: *const c_char,
                srv_items: *const NzbgetRsServerMatchItem,
                srv_len: usize,
                vol_items: *const NzbgetRsDiskVolumeStatInput,
                vol_len: usize,
            ) {
                let so = unsafe { &mut *sink_ctx.cast::<StatsSaveOut>() };
                if srv_items.is_null() || srv_len == 0 {
                    return;
                }
                so.has_servers = true;
                let srvs = unsafe { std::slice::from_raw_parts(srv_items, srv_len) };
                let names: Vec<*const c_char> = srvs.iter().map(|s| s.name).collect();
                let hosts: Vec<*const c_char> = srvs.iter().map(|s| s.host).collect();
                let ports: Vec<c_int> = srvs.iter().map(|s| s.port).collect();
                let users: Vec<*const c_char> = srvs.iter().map(|s| s.user).collect();
                so.srv_bytes = unsafe {
                    format_server_info_bytes(
                        names.as_ptr(),
                        hosts.as_ptr(),
                        ports.as_ptr(),
                        users.as_ptr(),
                        srvs.len(),
                    )
                };
                if !vol_items.is_null() && vol_len > 0 {
                    so.vol_count = vol_len;
                    for i in 0..vol_len {
                        let b =
                            unsafe { format_volume_stat_item_bytes(vol_items.add(i)) };
                        so.vol_bytes.extend_from_slice(&b);
                    }
                }
            }
            let mut so = StatsSaveOut {
                has_servers: false,
                srv_bytes: Vec::new(),
                vol_count: 0,
                vol_bytes: Vec::new(),
            };
            if let Some(aux) = env.aux_cb {
                unsafe {
                    aux(
                        env.ctx,
                        4,
                        1,
                        0,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        (&mut so as *mut StatsSaveOut).cast(),
                        Some(stats_save_sink),
                    );
                }
            }
            let sf = RsStateFile::new(
                qdir,
                sep,
                b"stats",
                DISKSTATE_STATS_VERSION,
                true,
                DISKSTATE_STATS_VERSION,
            );
            if !so.has_servers {
                sf.discard();
                return 1;
            }
            let Some(mut w) = (unsafe { sf.begin_write(env) }) else {
                return 0;
            };
            unsafe { emit_log(env, 0, b"Saving server info to disk") };
            let _ = w.write_all(&so.srv_bytes);
            unsafe { emit_log(env, 0, b"Saving volume stats to disk") };
            let _ = writeln!(w, "{}", so.vol_count as c_int);
            let _ = w.write_all(&so.vol_bytes);
            c_int::from(unsafe { sf.finish_write(env, w, env.flush_queue != 0) })
        }
        12 => {
            unsafe { emit_log(env, 0, b"Loading stats from disk") };
            let mut sf = RsStateFile::new(
                qdir,
                sep,
                b"stats",
                DISKSTATE_STATS_VERSION,
                true,
                DISKSTATE_STATS_VERSION,
            );
            if !sf.file_exists() {
                return 1;
            }
            let Some(mut r) = (unsafe { sf.begin_read(env) }) else {
                return 0;
            };
            let ver = sf.file_version;
            unsafe { emit_log(env, 0, b"Loading server info from disk") };
            struct SrvMeta {
                id: c_int,
                port: c_int,
                name: Vec<u8>,
                host: Vec<u8>,
                user: Vec<u8>,
            }
            unsafe extern "C" fn srv_load_sink(
                sink_ctx: *mut c_void,
                _kind: c_int,
                _a1: c_int,
                _a2: c_int,
                _u1: u32,
                _s1: *const c_char,
                srv_items: *const NzbgetRsServerMatchItem,
                srv_len: usize,
                _vol: *const NzbgetRsDiskVolumeStatInput,
                _vol_len: usize,
            ) {
                let vec = unsafe { &mut *sink_ctx.cast::<Vec<SrvMeta>>() };
                if !srv_items.is_null() && srv_len > 0 {
                    for s in unsafe { std::slice::from_raw_parts(srv_items, srv_len) } {
                        vec.push(SrvMeta {
                            id: s.id,
                            port: s.port,
                            name: unsafe { cstr_to_bytes(s.name) }.to_vec(),
                            host: unsafe { cstr_to_bytes(s.host) }.to_vec(),
                            user: unsafe { cstr_to_bytes(s.user) }.to_vec(),
                        });
                    }
                }
            }
            let mut srvs: Vec<SrvMeta> = Vec::new();
            if let Some(aux) = env.aux_cb {
                unsafe {
                    aux(
                        env.ctx,
                        4,
                        0,
                        0,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        (&mut srvs as *mut Vec<SrvMeta>).cast(),
                        Some(srv_load_sink),
                    );
                }
            }
            let inputs: Vec<ServerMatchInput<'_>> = srvs
                .iter()
                .map(|s| ServerMatchInput {
                    name: &s.name,
                    host: &s.host,
                    port: s.port,
                    user: &s.user,
                })
                .collect();
            let mut state_ids = vec![0i32; inputs.len()];
            let Some(perfect) = (unsafe {
                load_server_refs_and_match(
                    &inputs,
                    r.as_ctx(),
                    RsStateReader::read_line_cb,
                    &mut state_ids,
                )
            }) else {
                unsafe {
                    emit_log(env, 4, b"Error reading server info from disk");
                    emit_log(env, 4, b"Error reading diskstate for statistics");
                }
                return 0;
            };
            if !env.out_flag.is_null() {
                unsafe { *env.out_flag = c_int::from(perfect) };
            }
            if let Some(aux) = env.aux_cb {
                for (idx, &st_id) in state_ids.iter().enumerate() {
                    unsafe {
                        aux(
                            env.ctx,
                            5,
                            idx as c_int,
                            st_id,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            None,
                        );
                    }
                }
            }
            unsafe { emit_log(env, 0, b"******** MATCHING NEWS-SERVERS **********") };
            for (idx, s) in srvs.iter().enumerate() {
                unsafe {
                    emit_log(
                        env,
                        0,
                        format!("Server {} -> {}", s.id, state_ids[idx]).as_bytes(),
                    );
                    emit_log_parts(
                        env,
                        0,
                        &[format!("Server {}.Name: ", s.id).as_bytes(), &s.name],
                    );
                    emit_log_parts(
                        env,
                        0,
                        &[
                            format!("Server {}.Host: ", s.id).as_bytes(),
                            &s.host,
                            format!(":{}", s.port).as_bytes(),
                        ],
                    );
                }
            }
            unsafe {
                emit_log(
                    env,
                    0,
                    if perfect {
                        b"All servers perfectly matched: yes"
                    } else {
                        b"All servers perfectly matched: no"
                    },
                );
            }

            if ver >= 2 {
                unsafe { emit_log(env, 0, b"Loading volume stats from disk") };
                struct VolCtx<'a> {
                    env: &'a NzbgetRsDiskStateEnv,
                    ver: i32,
                }
                unsafe extern "C" fn on_vol(
                    ctx: *mut c_void,
                    slot_idx: c_int,
                    item: *const NzbgetRsDiskVolumeStatInput,
                    has_custom_times: c_int,
                ) {
                    let vc = unsafe { &*ctx.cast::<VolCtx<'_>>() };
                    if let Some(aux) = vc.env.aux_cb {
                        unsafe {
                            aux(
                                vc.env.ctx,
                                6,
                                slot_idx,
                                (vc.ver << 1) | (has_custom_times & 1),
                                0,
                                std::ptr::null(),
                                std::ptr::null(),
                                item,
                                std::ptr::null_mut(),
                                None,
                            );
                        }
                    }
                }
                let mut vc = VolCtx { env, ver };
                if !unsafe {
                    load_volume_stat(
                        r.as_ctx(),
                        RsStateReader::read_line_cb,
                        ver,
                        (&mut vc as *mut VolCtx<'_>).cast(),
                        on_vol,
                    )
                } {
                    unsafe {
                        emit_log(env, 4, b"Error reading volume stats from disk");
                        emit_log(env, 4, b"Error reading diskstate for statistics");
                    }
                    return 0;
                }
            }
            1
        }
        13 => c_int::from(unsafe {
            save_single_file_or_state(env, qdir, sep, env.a1, env.a2 != 0)
        }),
        14 => {
            let mut ver = 0;
            let ok = unsafe {
                load_single_file_or_state(
                    env,
                    qdir,
                    tdir,
                    sep,
                    env.a1,
                    env.a2 != 0,
                    env.a3 != 0,
                    &mut ver,
                )
            };
            if !env.out_flag.is_null() && env.a1 != 0 && ver != 0 {
                unsafe { *env.out_flag = ver };
            }
            c_int::from(ok)
        }
        15 => {
            let flush_queue = env.flush_queue != 0;
            if env.a1 == 0 {
                unsafe { emit_log(env, 0, b"Saving queue and history to disk") };
                let mut ok = true;
                let queue_len = unsafe { call_ctrl(env, 0, 0, 0, 0, std::ptr::null()) };
                let q_sf = RsStateFile::new(
                    qdir,
                    sep,
                    b"queue",
                    DISKSTATE_QUEUE_VERSION,
                    true,
                    DISKSTATE_QUEUE_VERSION,
                );
                if queue_len > 0 {
                    let Some(mut w) = (unsafe { q_sf.begin_write(env) }) else {
                        return 0;
                    };
                    unsafe {
                        emit_log(env, 0, b"Saving nzb list to disk");
                        save_nzb_section(env, &mut w, 0, queue_len);
                    }
                    ok = unsafe { q_sf.finish_write(env, w, flush_queue) };
                } else {
                    q_sf.discard();
                }

                if env.a2 != 0 {
                    let hist_len = unsafe { call_ctrl(env, 1, 0, 0, 0, std::ptr::null()) };
                    let h_sf = RsStateFile::new(
                        qdir,
                        sep,
                        b"history",
                        DISKSTATE_QUEUE_VERSION,
                        true,
                        DISKSTATE_QUEUE_VERSION,
                    );
                    if hist_len > 0 {
                        let Some(mut w) = (unsafe { h_sf.begin_write(env) }) else {
                            return 0;
                        };
                        unsafe {
                            emit_log(env, 0, b"Saving history to disk");
                            save_nzb_section(env, &mut w, 1, hist_len);
                        }
                        ok &= unsafe { h_sf.finish_write(env, w, flush_queue) };
                    } else {
                        h_sf.discard();
                    }
                }

                let p_sf = RsStateFile::new(
                    qdir,
                    sep,
                    b"progress",
                    DISKSTATE_QUEUE_VERSION,
                    true,
                    DISKSTATE_QUEUE_VERSION,
                );
                p_sf.discard();

                let last_nzb = unsafe { call_ctrl(env, 2, 0, 0, 0, std::ptr::null()) };
                let last_file = unsafe { call_ctrl(env, 3, 0, 0, 0, std::ptr::null()) };
                let _ = save_id_floor_file(qdir, sep, last_nzb, last_file);
                c_int::from(ok)
            } else if env.a1 == 1 {
                let changed_count = unsafe { call_ctrl(env, 20, 0, 0, 0, std::ptr::null()) };
                unsafe { emit_log(env, 0, b"Saving queue progress to disk") };
                let p_sf = RsStateFile::new(
                    qdir,
                    sep,
                    b"progress",
                    DISKSTATE_QUEUE_VERSION,
                    true,
                    DISKSTATE_QUEUE_VERSION,
                );
                if changed_count > 0 {
                    let Some(mut w) = (unsafe { p_sf.begin_write(env) }) else {
                        return 0;
                    };
                    unsafe {
                        emit_log(env, 0, b"Saving nzb progress to disk");
                        save_nzb_section(env, &mut w, 2, changed_count);
                    }
                    c_int::from(unsafe { p_sf.finish_write(env, w, flush_queue) })
                } else {
                    p_sf.discard();
                    1
                }
            } else {
                let nzb_count = unsafe { call_ctrl(env, 0, 0, 0, 0, std::ptr::null()) };
                let f_sf = RsStateFile::new(
                    qdir,
                    sep,
                    b"files",
                    DISKSTATE_FILE_VERSION,
                    true,
                    DISKSTATE_FILE_VERSION,
                );
                if nzb_count > 0 {
                    let Some(mut w) = (unsafe { f_sf.begin_write(env) }) else {
                        return 0;
                    };
                    let total_files = unsafe { call_ctrl(env, 4, 0, 0, 0, std::ptr::null()) };
                    let _ = writeln!(w, "{total_files}");
                    for nzb_idx in 0..nzb_count {
                        let fl_len =
                            unsafe { call_ctrl(env, 5, nzb_idx, 0, 0, std::ptr::null()) };
                        for file_idx in 0..fl_len {
                            let file_id = unsafe {
                                call_ctrl(env, 6, nzb_idx, file_idx, 0, std::ptr::null())
                            };
                            let _ = writeln!(w, "{file_id}");
                            let (_, bytes) = unsafe { format_target_file_bytes(env, 1) };
                            let _ = w.write_all(&bytes);
                        }
                    }
                    c_int::from(unsafe { f_sf.finish_write(env, w, flush_queue) })
                } else {
                    f_sf.discard();
                    1
                }
            }
        }
        16 => {
            unsafe { emit_log(env, 0, b"Loading queue from disk") };
            let mut queue_files_failed = false;
            let mut file_infos_loaded = false;
            let mut format_version = 0i32;
            let mut legacy_queue_read = false;

            let ok = 'load: {
                let mut q_sf = RsStateFile::new(
                    qdir,
                    sep,
                    b"queue",
                    DISKSTATE_QUEUE_VERSION,
                    true,
                    DISKSTATE_QUEUE_LEGACY_VERSION,
                );
                if q_sf.file_exists() {
                    let Some(mut r) = (unsafe { q_sf.begin_read(env) }) else {
                        queue_files_failed = true;
                        break 'load false;
                    };
                    format_version = q_sf.file_version;
                    let (ver_err, is_legacy, need_inline_hist) =
                        check_queue_format_version(format_version, DISKSTATE_QUEUE_VERSION);
                    legacy_queue_read |= is_legacy;
                    if ver_err == 1 {
                        unsafe {
                            emit_log(env, 4, b"Failed to read queue: diskstate file is corrupted");
                        }
                        queue_files_failed = true;
                        break 'load false;
                    } else if ver_err == 2 {
                        unsafe {
                            emit_log(
                                env,
                                4,
                                b"Failed to read queue and history data. Only queue and history from NZBGet v13 or newer can be converted by this RustyGet version. Old queue and history data still can be converted using NZBGet v16 as an intermediate version.",
                            );
                        }
                        queue_files_failed = true;
                        break 'load false;
                    }
                    if !unsafe { load_queue_section(env, &mut r, format_version) } {
                        queue_files_failed = true;
                        break 'load false;
                    }
                    if need_inline_hist
                        && !unsafe { load_history_section(env, &mut r, format_version) }
                    {
                        queue_files_failed = true;
                        break 'load false;
                    }
                }

                let mut p_sf = RsStateFile::new(
                    qdir,
                    sep,
                    b"progress",
                    DISKSTATE_QUEUE_VERSION,
                    true,
                    DISKSTATE_QUEUE_LEGACY_VERSION,
                );
                if p_sf.file_exists() {
                    let Some(mut r) = (unsafe { p_sf.begin_read(env) }) else {
                        queue_files_failed = true;
                        break 'load false;
                    };
                    if p_sf.file_version <= 0 {
                        unsafe {
                            emit_log(env, 4, b"Failed to read queue: diskstate file is corrupted");
                        }
                        queue_files_failed = true;
                        break 'load false;
                    }
                    legacy_queue_read |= p_sf.file_version != DISKSTATE_QUEUE_VERSION;
                    if !unsafe { load_progress_section(env, &mut r, p_sf.file_version) } {
                        queue_files_failed = true;
                        break 'load false;
                    }
                }

                if format_version == 0 || format_version >= 57 {
                    let mut h_sf = RsStateFile::new(
                        qdir,
                        sep,
                        b"history",
                        DISKSTATE_QUEUE_VERSION,
                        true,
                        DISKSTATE_QUEUE_LEGACY_VERSION,
                    );
                    if h_sf.file_exists() {
                        let Some(mut r) = (unsafe { h_sf.begin_read(env) }) else {
                            queue_files_failed = true;
                            break 'load false;
                        };
                        if h_sf.file_version <= 0 {
                            unsafe {
                                emit_log(
                                    env,
                                    4,
                                    b"Failed to read queue: diskstate file is corrupted",
                                );
                            }
                            queue_files_failed = true;
                            break 'load false;
                        }
                        legacy_queue_read |= h_sf.file_version != DISKSTATE_QUEUE_VERSION;
                        if !unsafe { load_history_section(env, &mut r, h_sf.file_version) } {
                            queue_files_failed = true;
                            break 'load false;
                        }
                    }
                }

                unsafe { load_all_file_infos_op(env, qdir, sep) };
                file_infos_loaded = true;

                unsafe { cleanup_queue_dir_op(env, qdir, sep) };

                if !unsafe { load_all_file_states_op(env, qdir, tdir, sep) } {
                    break 'load false;
                }

                unsafe { convert_legacy_file_states_op(env, qdir, tdir, sep) };
                true
            };

            if legacy_queue_read && !env.out_flag.is_null() {
                unsafe { *env.out_flag = 1 };
            }
            if !ok {
                unsafe {
                    emit_log(
                        env,
                        4,
                        b"Error reading diskstate for download queue and history",
                    );
                }
            }
            if !file_infos_loaded
                && unsafe { call_ctrl(env, 0, 0, 0, 0, std::ptr::null()) > 0 }
            {
                unsafe { load_all_file_infos_op(env, qdir, sep) };
            }
            if queue_files_failed {
                unsafe { set_aside_queue_files_op(env, qdir, sep) };
            }
            let mut max_nzb = -1;
            let mut max_file = -1;
            diskstate_fs_op(4, qdir, sep, 0, 0, 0, 0, &mut max_nzb, &mut max_file);
            unsafe { call_ctrl(env, 18, max_nzb, max_file, 0, std::ptr::null()) };
            c_int::from(ok)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn vec_to_string(b: Vec<u8>) -> String {
        String::from_utf8(b).unwrap()
    }

    #[test]
    fn test_format_dup_info_sanitizes_newlines() {
        unsafe {
            let name = CString::new("Release\nName").unwrap();
            let key = CString::new("series=1\r2").unwrap();
            let s = vec_to_string(format_dup_info_bytes(
                1,
                (2i64 << 32) | 5,
                100,
                200,
                1000,
                0,
                name.as_ptr(),
                key.as_ptr(),
            ));
            assert_eq!(s, "1,2,5,100,200,1000,0\nRelease Name\nseries=1 2\n");
        }
    }

    #[test]
    fn test_format_file_state_roundtrip_layout() {
        unsafe {
            let fname = CString::new("part01.rar").unwrap();
            let h16k = CString::new("deadbeef").unwrap();
            let pset = CString::new("parset1").unwrap();
            let inp = NzbgetRsDiskFileStateInput {
                success_articles: 10,
                failed_articles: 1,
                remaining_size: 1024,
                success_size: 2048,
                failed_size: 512,
                filename: fname.as_ptr(),
                hash16k: h16k.as_ptr(),
                par_set_id: pset.as_ptr(),
                par_file: 0,
                decoded_file_size: 4096,
                output_initialized: 1,
            };
            let stat = NzbgetRsDiskServerStat {
                server_id: 1,
                success_articles: 10,
                failed_articles: 1,
            };
            let art = NzbgetRsDiskArticleState {
                saved_status: 2,
                segment_offset: 0,
                segment_size: 512,
                crc: 0x12345678,
                staged_fallback: 0,
            };
            let s = vec_to_string(format_file_state_bytes(&inp, &stat, 1, &art, 1));
            assert!(s.contains("10,1\n0,1024,0,2048,0,512\npart01.rar\ndeadbeef\nparset1\n0\n"));
            assert!(s.contains("#decoded 0,4096\nOutputInitialized=1\n"));
        }
    }

    #[test]
    fn test_parse_completed_file_line_and_formatters() {
        unsafe {
            let mut line = b"42,1,305419896,0,deadbeef,set1,movie.mkv\0".to_vec();
            let mut id = 0;
            let mut status = 0;
            let mut crc = 0u32;
            let mut par = 0;
            let mut h16k: *const c_char = std::ptr::null();
            let mut pset: *const c_char = std::ptr::null();
            let mut fname: *const c_char = std::ptr::null();
            assert_eq!(
                parse_completed_file_line(
                    line.as_mut_ptr().cast(),
                    61,
                    &mut id,
                    &mut status,
                    &mut crc,
                    &mut par,
                    &mut h16k,
                    &mut pset,
                    &mut fname,
                ),
                1
            );
            assert_eq!((id, status, crc, par), (42, 1, 305419896, 0));
            assert_eq!(std::ffi::CStr::from_ptr(h16k).to_bytes(), b"deadbeef");
            assert_eq!(std::ffi::CStr::from_ptr(pset).to_bytes(), b"set1");
            assert_eq!(std::ffi::CStr::from_ptr(fname).to_bytes(), b"movie.mkv");

            let a1 = CString::new("a\n.rar").unwrap();
            let a2 = CString::new("b\r.rar").unwrap();
            let arcs = [a1.as_ptr(), a2.as_ptr()];
            let trailer = vec_to_string(format_unpack_trailer_item_bytes(
                7,
                2,
                1,
                1,
                3,
                arcs.as_ptr(),
                2,
            ));
            assert_eq!(
                trailer,
                "7\nDirectUnpackStatus=2\nUnpackTried=3\nLastUnpackStatus=3\nExtractedArchives=a .rar,b .rar\n"
            );

            let url = CString::new("https://example.invalid/feed\n1\r2").unwrap();
            let urls = [url.as_ptr()];
            let hashes = [12345u32];
            let updates = [1700000000];
            let feed_status = vec_to_string(format_feed_status_bytes(
                urls.as_ptr(),
                hashes.as_ptr(),
                updates.as_ptr(),
                1,
            ));
            assert_eq!(feed_status, "1\nhttps://example.invalid/feed 1 2\n12345\n1700000000\n");

            let statuses = [2];
            let seens = [1700000001];
            let feed_hist = vec_to_string(format_feed_history_bytes(
                statuses.as_ptr(),
                seens.as_ptr(),
                urls.as_ptr(),
                1,
            ));
            assert_eq!(feed_hist, "1\n2,1700000001\nhttps://example.invalid/feed 1 2\n");

            let srv_name = CString::new("Server\n1").unwrap();
            let srv_host = CString::new("news.example\r.com").unwrap();
            let srv_user = CString::new("user\nname").unwrap();
            let names = [srv_name.as_ptr()];
            let hosts = [srv_host.as_ptr()];
            let ports = [563];
            let users = [srv_user.as_ptr()];
            let srv_info = vec_to_string(format_server_info_bytes(
                names.as_ptr(),
                hosts.as_ptr(),
                ports.as_ptr(),
                users.as_ptr(),
                1,
            ));
            assert_eq!(srv_info, "1\nServer 1\nnews.example .com\n563\nuser name\n");
        }
    }

    #[test]
    fn test_state_reader_overlong_scroll_and_state_file_recovery() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "rustyget_statefile_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let qdir_bytes = dir.as_os_str().as_bytes().to_vec();
        let qdir_c = bytes_to_cstring_trunc(&qdir_bytes);
        let logs: std::cell::RefCell<Vec<(c_int, Vec<u8>)>> = std::cell::RefCell::new(Vec::new());

        unsafe extern "C" fn log_cb(ctx: *mut c_void, level: c_int, msg: *const c_char) {
            let v = unsafe { &*ctx.cast::<std::cell::RefCell<Vec<(c_int, Vec<u8>)>>>() };
            v.borrow_mut()
                .push((level, unsafe { cstr_to_bytes(msg) }.to_vec()));
        }

        let mut env: NzbgetRsDiskStateEnv = unsafe { std::mem::zeroed() };
        env.ctx = &logs as *const _ as *mut c_void;
        env.queue_dir = qdir_c.as_ptr();
        env.sep = b'/' as c_int;
        env.flush_queue = 1;
        env.now = 1700000099;
        env.log_cb = Some(log_cb);

        // 1. Transactional write, fsync, and read-only destination overwrite (replace=true)
        unsafe {
            let sf = RsStateFile::new(&qdir_bytes, b'/', b"77s", 7, true, 7);
            let mut w = sf.begin_write(&env).unwrap();
            writeln!(w, "{}", "x".repeat(300)).unwrap();
            writeln!(w, "after_overlong").unwrap();
            assert!(sf.finish_write(&env, w, true));

            let dest_path = dir.join("77s");
            let mut perms = std::fs::metadata(&dest_path).unwrap().permissions();
            perms.set_mode(0o444);
            std::fs::set_permissions(&dest_path, perms).unwrap();

            let mut sf_rep = RsStateFile::new(&qdir_bytes, b'/', b"77s", 7, true, 7);
            sf_rep.flush = false;
            let mut w2 = sf_rep.begin_write(&env).unwrap();
            writeln!(w2, "{}", "y".repeat(256 * 1024)).unwrap();
            writeln!(w2, "second_line").unwrap();
            assert!(sf_rep.finish_write(&env, w2, true));

            let mut sf_rd = RsStateFile::new(&qdir_bytes, b'/', b"77s", 7, false, 9);
            let mut r = sf_rd.begin_read(&env).unwrap();
            assert_eq!(sf_rd.file_version, 7);
            let cut = r.read_line(32).unwrap().to_vec();
            assert_eq!(cut, vec![b'y'; 30]);
            assert!(r.raw_buf.capacity() <= 1024);
            let next = r.read_line(128).unwrap();
            assert_eq!(next, b"second_line");
            assert!(r.read_line(128).is_none());
        }

        // 2. Disaster recovery from .new when dest is missing, and version mismatch refusal
        unsafe {
            std::fs::write(
                dir.join("queue.new"),
                b"nzbget diskstate file version 64\n0\n",
            )
            .unwrap();
            let mut sf_rec = RsStateFile::new(&qdir_bytes, b'/', b"queue", 64, true, 66);
            assert!(sf_rec.file_exists());
            let mut r = sf_rec.begin_read(&env).unwrap();
            assert_eq!(r.scan_i32(), Some(0));
            assert!(dir.join("queue").exists());
            assert!(!dir.join("queue.new").exists());

            std::fs::write(dir.join("queue"), b"nzbget diskstate file version 99\n0\n").unwrap();
            let mut out_legacy = 0;
            env.out_flag = &mut out_legacy;
            assert_eq!(diskstate_exec(16, &env), 0);
            assert!(dir.join("queue.unreadable-1700000099").exists());
            assert!(!dir.join("queue").exists());
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_diskstate_format_and_ids() {
        assert_eq!(
            parse_format_version(b"nzbget diskstate file version 64"),
            64
        );
        assert_eq!(parse_format_version(b"invalid header"), 0);

        let mut line = b"hello\r\nworld\n".to_vec();
        sanitize_line_breaks_in_place(&mut line);
        assert_eq!(&line, b"hello  world ");

        assert_eq!(parse_ids_file(b"nzb 42 file 1080\n"), Some((42, 1080)));
        assert_eq!(parse_ids_file(b"nzb -1 file 5"), None);

        assert_eq!(classify_queue_dir_id(b"n15.log"), Some((15, true)));
        assert_eq!(classify_queue_dir_id(b"204"), Some((204, false)));
        assert_eq!(classify_queue_dir_id(b"204s"), Some((204, false)));
        assert_eq!(classify_queue_dir_id(b"204c"), Some((204, false)));
        assert_eq!(classify_queue_dir_id(b"204x"), None);
    }

    #[test]
    fn test_diskstate_legacy_and_holes() {
        assert_eq!(
            classify_legacy_file_state(
                b"123c",
                b"nzbget diskstate file version 8\n",
                7,
                9
            ),
            Some((123, 2))
        );
        assert_eq!(
            classify_legacy_file_state(
                b"123c",
                b"nzbget diskstate file version 7\n",
                7,
                9
            ),
            None
        );

        assert_eq!(validate_repair_hole(0, 100, 0, 50, 200), Some((100, 50)));
        assert_eq!(validate_repair_hole(0, 160, 0, 50, 200), None);

        assert_eq!(
            parse_unpack_state_lines(
                b"DirectUnpackStatus=1",
                b"UnpackTried=3",
                b"LastUnpackStatus=2",
                1,
                0
            ),
            Some((0, true, true, 2))
        );
        assert_eq!(normalize_post_stage(6, 58, 12), 7);
        assert_eq!(normalize_post_stage(7, 58, 12), 9);
        assert_eq!(normalize_post_stage(14, 64, 12), 12);
    }

    #[test]
    fn test_diskstate_article_and_cleanup() {
        let eval = eval_article_load_state(
            7, false, 5, 0, 1024, 512, 1, 1, true, true, false, false, 0, 10,
        );
        assert_eq!(eval.valid, 1);
        assert_eq!(eval.status, 2);
        assert_eq!(eval.dupe_fallback_round, 1);
        assert_eq!(eval.need_result_filename, 1);
        assert_eq!(eval.completed_articles, 1);

        assert_eq!(classify_temp_file(b"foo.tmp"), (1, 0, 0));
        assert_eq!(classify_temp_file(b"12.003"), (2, 12, 3));
        assert_eq!(classify_temp_file(b"readme.txt"), (0, 0, 0));

        let file_ids = [10, 20, 30];
        let nzb_ids = [1, 2];
        assert!(!is_orphan_queue_file(b"20s", &file_ids, &nzb_ids));
        assert!(is_orphan_queue_file(b"25s", &file_ids, &nzb_ids));
        assert!(!is_orphan_queue_file(b"n2.log", &file_ids, &nzb_ids));
        assert!(is_orphan_queue_file(b"n3.log", &file_ids, &nzb_ids));
    }

    #[test]
    fn test_diskstate_match_servers_and_logs() {
        let servers = [
            ServerMatchInput {
                name: b"Main",
                host: b"news.example.com",
                port: 563,
                user: b"alice",
            },
            ServerMatchInput {
                name: b"RenamedBackup",
                host: b"backup.example.com",
                port: 563,
                user: b"bob",
            },
        ];
        let mut refs = [
            ServerRefEntry {
                state_id: 1,
                name: b"Main",
                host: b"news.example.com",
                port: 563,
                user: b"alice",
                matched: false,
                perfect: false,
            },
            ServerRefEntry {
                state_id: 2,
                name: b"OldBackup",
                host: b"backup.example.com",
                port: 563,
                user: b"bob",
                matched: false,
                perfect: false,
            },
        ];
        let mut out_ids = [0, 0];
        let perfect = match_servers(&servers, &mut refs, &mut out_ids);
        assert!(!perfect);
        assert_eq!(out_ids, [1, 2]);

        let parsed = parse_log_line(b"Tue Nov 3 2026\t1793730000\tWARNING\tDisk almost full");
        assert_eq!(parsed, Some((1793730000, 1, 34)));
        assert!(validate_dupe_extras_header(4, 0, 0));
        assert!(validate_repair_job_header(7, 7, 10, 0, 0));
        assert!(eval_restored_output_init(1, true, false, true, false, false));
        assert_eq!(calc_staged_fallback(false, 2, 2, 1, true, 0), (1, 0));
        assert!(validate_article_count(50, true, 100));
        assert!(need_staged_file_check(false, 9, 0, 2, 1024, 512, 1, 3, true, false));
        assert_eq!(calc_volume_slot_size(0, 10), (true, 60));
    }

    #[test]
    fn test_diskstate_fs_op_and_set_aside_regular_files() {
        let dir = std::env::temp_dir().join(format!(
            "rustyget_diskstate_fs_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let qdir = dir.to_string_lossy().into_owned().into_bytes();
        let mut o1 = 0;
        let mut o2 = 0;
        assert_eq!(diskstate_fs_op(1, &qdir, b'/', 0, 0, 0, 0, &mut o1, &mut o2), 0);
        assert_eq!(diskstate_fs_op(5, &qdir, b'/', 12, 34, 0, 0, &mut o1, &mut o2), 1);
        assert_eq!(diskstate_fs_op(1, &qdir, b'/', 0, 0, 0, 0, &mut o1, &mut o2), 1);
        assert_eq!(diskstate_fs_op(4, &qdir, b'/', 0, 0, 0, 0, &mut o1, &mut o2), 1);
        assert_eq!((o1, o2), (12, 34));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
