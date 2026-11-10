//! DiskState record serialization and helpers (`daemon/queue/DiskState.cpp`).

use std::ffi::{c_char, c_int};
use std::io::Write;

use crate::ffi::{input, into_buf, RsBuf};

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
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_nzb_info(
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
) -> RsBuf {
    if inp.is_null() {
        return into_buf(Vec::new());
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

    into_buf(out)
}

/// # Safety
/// `jobs` must be valid for `jobs_len` items.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_dupe_extras(
    recovered_articles: c_int,
    recovered_bytes: i64,
    recovered_holes: c_int,
    jobs: *const NzbgetRsDiskRepairJob,
    jobs_len: usize,
) -> RsBuf {
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
    into_buf(out)
}

/// # Safety
/// `inp`, `groups`, and `articles` must be valid for the call duration.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_file_info(
    inp: *const NzbgetRsDiskFileInfoInput,
    groups: *const *const c_char,
    groups_len: usize,
    include_articles: c_int,
    articles: *const NzbgetRsDiskArticleInfo,
    articles_len: usize,
) -> RsBuf {
    if inp.is_null() {
        return into_buf(Vec::new());
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

    into_buf(out)
}

/// # Safety
/// `inp`, `stats`, and `articles` must be valid for the call duration.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_file_state(
    inp: *const NzbgetRsDiskFileStateInput,
    stats: *const NzbgetRsDiskServerStat,
    stats_len: usize,
    articles: *const NzbgetRsDiskArticleState,
    articles_len: usize,
) -> RsBuf {
    if inp.is_null() {
        return into_buf(Vec::new());
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

    into_buf(out)
}

/// # Safety
/// `name` and `dupe_key` are null or valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_dup_info(
    status: c_int,
    size: i64,
    full_content_hash: u32,
    filtered_content_hash: u32,
    dupe_score: c_int,
    dupe_mode: c_int,
    name: *const c_char,
    dupe_key: *const c_char,
) -> RsBuf {
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
    into_buf(out)
}

/// # Safety
/// `inp` and all slice pointers must be valid for the call duration.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_volume_stat_item(
    inp: *const NzbgetRsDiskVolumeStatInput,
) -> RsBuf {
    if inp.is_null() {
        return into_buf(Vec::new());
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

    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_unpack_trailer_item(
    id: c_int,
    direct_unpack_status: c_int,
    unpack_tried: c_int,
    par_repaired: c_int,
    last_unpack_status: c_int,
    archives: *const *const c_char,
    archives_len: usize,
) -> RsBuf {
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
    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_feed_status(
    urls: *const *const c_char,
    filter_hashes: *const u32,
    last_updates: *const c_int,
    len: usize,
) -> RsBuf {
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
    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_feed_history(
    statuses: *const c_int,
    last_seens: *const c_int,
    urls: *const *const c_char,
    len: usize,
) -> RsBuf {
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
    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_server_info(
    names: *const *const c_char,
    hosts: *const *const c_char,
    ports: *const c_int,
    users: *const *const c_char,
    len: usize,
) -> RsBuf {
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
    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_format_nzb_message_line(
    time_str: *const c_char,
    tm: c_int,
    kind: c_int,
    text: *const c_char,
    line_ending: *const c_char,
) -> RsBuf {
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
    into_buf(out)
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
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_parse_completed_file_line(
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

/// Parse `%i` style integer (base 10, `0x` hex, or `0` octal).
fn parse_scanf_i32(bytes: &[u8]) -> Option<(i32, usize)> {
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
    Some((val as i32, digits_start))
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
        3 => {
            let Some(cb) = read_line_cb else { return false };
            let Some(stat_count) = (unsafe { scan_one_i32(infile_ctx, cb) }) else {
                return false;
            };
            for _ in 0..stat_count {
                let Some(line) = (unsafe { read_line_bytes(infile_ctx, cb, 1024) }) else {
                    return false;
                };
                let mut sc = ScanCursor(&line);
                let (Some(server_id), Some(success), Some(failed)) =
                    (sc.i32(false), sc.i32(true), sc.i32(true))
                else {
                    return false;
                };
                unsafe {
                    on_entry_cb(
                        target_ctx,
                        server_id,
                        success,
                        failed,
                        0,
                        0,
                        0,
                        std::ptr::null(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    unsafe fn buf_to_string(b: RsBuf) -> String {
        let s = String::from_utf8_lossy(std::slice::from_raw_parts(b.data as *const u8, b.len)).into_owned();
        crate::ffi::nzbget_rs_free(b);
        s
    }

    #[test]
    fn test_format_dup_info_sanitizes_newlines() {
        unsafe {
            let name = CString::new("Release\nName").unwrap();
            let key = CString::new("series=1\r2").unwrap();
            let s = buf_to_string(nzbget_rs_diskstate_format_dup_info(
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
            let s = buf_to_string(nzbget_rs_diskstate_format_file_state(&inp, &stat, 1, &art, 1));
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
                nzbget_rs_diskstate_parse_completed_file_line(
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
            let trailer = buf_to_string(nzbget_rs_diskstate_format_unpack_trailer_item(
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
            let feed_status = buf_to_string(nzbget_rs_diskstate_format_feed_status(
                urls.as_ptr(),
                hashes.as_ptr(),
                updates.as_ptr(),
                1,
            ));
            assert_eq!(feed_status, "1\nhttps://example.invalid/feed 1 2\n12345\n1700000000\n");

            let statuses = [2];
            let seens = [1700000001];
            let feed_hist = buf_to_string(nzbget_rs_diskstate_format_feed_history(
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
            let srv_info = buf_to_string(nzbget_rs_diskstate_format_server_info(
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
