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

    let persisted_stage =
        crate::ffi::nzbget_rs_dps_diskstate_calc_persisted_post_stage(n.has_post_info, n.post_stage);
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
}
