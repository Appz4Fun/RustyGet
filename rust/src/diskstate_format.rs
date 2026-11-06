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
    let unpack_tried_flags = crate::diskstatepp::pack_unpack_tried_flags(
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
    crate::diskstatepp::sanitize_log_text_in_place(&mut clean_text);
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
}
