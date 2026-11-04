//! Formatting routines for `NCursesFrontend` (`daemon/frontend/NCursesFrontend.cpp`):
//! status bar, top header with uptime/download-time suffix, file queue header and rows,
//! and group queue header, column width calculation, and rows.

const MAX_SCREEN_WIDTH: usize = 512;
const BSTRING_1024_CAP: usize = 1023;

fn push_two_digits(out: &mut Vec<u8>, val: i32) {
    if (0..10).contains(&val) {
        out.push(b'0');
        out.push(b'0' + val as u8);
    } else if (-9..0).contains(&val) {
        out.extend_from_slice(b"-0");
        out.push(b'0' + (-val) as u8);
    } else {
        out.extend_from_slice(val.to_string().as_bytes());
    }
}

fn push_hms(out: &mut Vec<u8>, h: i32, m: i32, s: i32) {
    push_two_digits(out, h);
    out.push(b':');
    push_two_digits(out, m);
    out.push(b':');
    push_two_digits(out, s);
}

fn push_hm(out: &mut Vec<u8>, h: i32, m: i32) {
    push_two_digits(out, h);
    out.push(b':');
    push_two_digits(out, m);
}

fn truncate_bstring_1024(mut v: Vec<u8>) -> Vec<u8> {
    if v.len() > BSTRING_1024_CAP {
        v.truncate(BSTRING_1024_CAP);
    }
    v
}

/// FFI descriptor for `NCursesFrontend::PrintStatus`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NcursesStatusSpec {
    pub stand_by: i32,
    pub pause_download: i32,
    pub current_download_speed: i64,
    pub remaining_size: i64,
    pub download_limit: i64,
    pub post_job_count: i32,
    pub dn_time_sec: i32,
    pub all_bytes: i64,
    pub thread_count: i32,
}

/// Format the bottom status bar line (`NCursesFrontend::PrintStatus`).
pub fn format_status_bar(spec: &NcursesStatusSpec) -> Vec<u8> {
    let mut out = Vec::with_capacity(160);
    let cur_speed = if spec.stand_by != 0 {
        0
    } else {
        spec.current_download_speed
    };

    out.push(b' ');
    out.extend_from_slice(spec.thread_count.to_string().as_bytes());
    out.extend_from_slice(b" threads, ");
    out.extend_from_slice(&crate::util::format_speed(cur_speed));
    out.extend_from_slice(b", ");
    crate::util::format_size_into(&mut out, spec.remaining_size);
    out.extend_from_slice(b" remaining");

    if cur_speed > 0 && spec.pause_download == 0 {
        let remain_sec = spec.remaining_size / cur_speed;
        let (h, m, s) = crate::connclientmain::remote_client_calc_hms(remain_sec);
        out.extend_from_slice(b" (~ ");
        push_hms(&mut out, h, m, s);
        out.push(b')');
    }

    if spec.post_job_count > 0 {
        out.extend_from_slice(b", ");
        out.extend_from_slice(spec.post_job_count.to_string().as_bytes());
        out.extend_from_slice(if spec.post_job_count > 1 {
            b" post-jobs"
        } else {
            b" post-job"
        });
    }

    let pause_suffix = crate::schedfrontweb::frontend_select_pause_status_suffix(
        spec.pause_download != 0,
        spec.stand_by != 0,
    );
    out.extend_from_slice(pause_suffix.to_bytes());

    if spec.download_limit > 0 {
        let limit_kb = crate::schedfrontweb::frontend_calc_limit_kb(spec.download_limit as i32);
        out.extend_from_slice(b", Limit ");
        out.extend_from_slice(limit_kb.to_string().as_bytes());
        out.extend_from_slice(b" KB/s");
    }

    let avg_speed = if spec.dn_time_sec > 0 {
        spec.all_bytes / (spec.dn_time_sec as i64)
    } else {
        0
    };
    out.extend_from_slice(b", Avg. ");
    out.extend_from_slice(&crate::util::format_speed(avg_speed));

    truncate_bstring_1024(out)
}

/// Format the top header bar with right-aligned uptime or download-time (`NCursesFrontend::PrintTopHeader`).
pub fn format_top_header(header: &[u8], screen_width: i32, up_time: bool, time_sec: i32) -> Vec<u8> {
    let width = screen_width.max(0) as usize;
    let mut buffer = Vec::with_capacity(width.max(header.len()).min(BSTRING_1024_CAP));
    buffer.extend_from_slice(header);
    if buffer.len() < width {
        buffer.resize(width, b' ');
    }
    buffer = truncate_bstring_1024(buffer);

    let header_len = header.len() as i32;
    let chars_left = screen_width.wrapping_sub(header_len).wrapping_sub(2);

    let (d, h, m, s) = crate::frontexthealth::ncurses_calc_top_header_time(time_sec);
    let mut time_str = Vec::with_capacity(32);
    if d == 0 {
        push_hms(&mut time_str, h, m, s);
        if (time_str.len() as i32) > chars_left {
            time_str.clear();
            push_hm(&mut time_str, h, m);
        }
    } else {
        time_str.extend_from_slice(d.to_string().as_bytes());
        time_str.extend_from_slice(if d == 1 { b" day " } else { b" days " });
        push_hms(&mut time_str, h, m, s);
        if (time_str.len() as i32) > chars_left {
            time_str.clear();
            time_str.extend_from_slice(d.to_string().as_bytes());
            time_str.extend_from_slice(b"d ");
            push_hms(&mut time_str, h, m, s);
        }
        if (time_str.len() as i32) > chars_left {
            time_str.clear();
            time_str.extend_from_slice(d.to_string().as_bytes());
            time_str.extend_from_slice(b"d ");
            push_hm(&mut time_str, h, m);
        }
    }

    let short_cap: &[u8] = if up_time { b" Up " } else { b"Dn " };
    let long_cap: &[u8] = if up_time {
        b" Uptime "
    } else {
        b" Download-time "
    };

    let time_len = time_str.len() as i32;
    let short_cap_len = short_cap.len() as i32;
    let long_cap_len = long_cap.len() as i32;

    let cap_mode = crate::schedfrontweb::ncurses_select_top_header_caption(
        chars_left,
        time_len,
        long_cap_len,
        short_cap_len,
    );

    let write_suffix = |buf: &mut Vec<u8>, start: i32, cap: &[u8], t: &[u8]| {
        if start < 0 {
            return;
        }
        let start_idx = start as usize;
        if start_idx >= MAX_SCREEN_WIDTH {
            return;
        }
        let max_avail = MAX_SCREEN_WIDTH - start_idx;
        if max_avail == 0 {
            return;
        }
        let mut combined = Vec::with_capacity(cap.len() + t.len());
        combined.extend_from_slice(cap);
        combined.extend_from_slice(t);
        if combined.len() >= max_avail {
            combined.truncate(max_avail - 1);
        }
        if buf.len() < start_idx + combined.len() {
            buf.resize(start_idx + combined.len(), b' ');
        }
        buf[start_idx..start_idx + combined.len()].copy_from_slice(&combined);
        buf.truncate(start_idx + combined.len());
    };

    if cap_mode == 2 {
        write_suffix(
            &mut buffer,
            screen_width - time_len - long_cap_len,
            long_cap,
            &time_str,
        );
    } else if cap_mode == 1 {
        write_suffix(
            &mut buffer,
            screen_width - time_len - short_cap_len,
            short_cap,
            &time_str,
        );
    } else if cap_mode == 0 {
        write_suffix(&mut buffer, screen_width - time_len, b"", &time_str);
    }

    truncate_bstring_1024(buffer)
}

/// Format the queue header for file mode (`NCursesFrontend::PrintFileQueue`).
pub fn format_file_queue_header(
    use_color: bool,
    file_num: i32,
    paused_files: i32,
    remaining: i64,
    paused: i64,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    if file_num > 0 {
        out.push(b' ');
        if !use_color {
            out.extend_from_slice(b"*** ");
        }
        out.extend_from_slice(b"Files for downloading - ");
        out.extend_from_slice(file_num.to_string().as_bytes());
        out.extend_from_slice(b" / ");
        out.extend_from_slice(file_num.wrapping_sub(paused_files).to_string().as_bytes());
        out.extend_from_slice(b" files in queue - ");
        crate::util::format_size_into(&mut out, remaining);
        out.extend_from_slice(b" / ");
        crate::util::format_size_into(&mut out, remaining.wrapping_sub(paused));
    } else {
        if !use_color {
            out.extend_from_slice(b"*** ");
        }
        out.extend_from_slice(b" Files for downloading");
    }
    truncate_bstring_1024(out)
}

/// Format the queue header for group mode (`NCursesFrontend::PrintGroupQueue`).
pub fn format_group_queue_header(
    use_color: bool,
    group_count: i32,
    remaining: i64,
    paused: i64,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    if group_count > 0 {
        out.push(b' ');
        if !use_color {
            out.extend_from_slice(b"*** ");
        }
        out.extend_from_slice(b"NZBs for downloading - ");
        out.extend_from_slice(group_count.to_string().as_bytes());
        out.extend_from_slice(b" NZBs in queue - ");
        crate::util::format_size_into(&mut out, remaining);
        out.extend_from_slice(b" / ");
        crate::util::format_size_into(&mut out, remaining.wrapping_sub(paused));
    } else {
        if !use_color {
            out.extend_from_slice(b"*** ");
        }
        out.extend_from_slice(b" NZBs for downloading");
    }
    truncate_bstring_1024(out)
}

/// FFI descriptor for `NCursesFrontend::PrintFilename`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NcursesFileRowSpec {
    pub edit_queue_mode: i32,
    pub selected: i32,
    pub use_color: i32,
    pub color_highl: i32,
    pub color_text: i32,
    pub id: i32,
    pub priority: i32,
    pub active_downloads: i32,
    pub paused: i32,
    pub show_nzbname: i32,
    pub path_sep: u8,
    pub nzb_name: *const u8,
    pub filename: *const u8,
    pub size: i64,
    pub remaining_size: i64,
}

unsafe fn cstr_bytes<'a>(ptr: *const u8) -> &'a [u8] {
    if ptr.is_null() {
        b""
    } else {
        std::ffi::CStr::from_ptr(ptr.cast()).to_bytes()
    }
}

/// Format a single file row in `NCursesFrontend::PrintFilename`, returning `(text, color)`.
pub fn format_file_row(spec: &NcursesFileRowSpec) -> (Vec<u8>, i32) {
    let (color, brace1, brace2) = crate::schedfrontweb::ncurses_select_row_highlight(
        spec.edit_queue_mode != 0,
        spec.selected != 0,
        spec.use_color != 0,
        spec.color_highl,
        spec.color_text,
    );

    let mut out = Vec::with_capacity(160);
    out.push(brace1);
    out.extend_from_slice(spec.id.to_string().as_bytes());
    out.push(brace2);

    if spec.priority != 0 {
        out.extend_from_slice(b" [");
        if spec.priority > 0 {
            out.push(b'+');
        }
        out.extend_from_slice(spec.priority.to_string().as_bytes());
        out.push(b']');
    }

    if spec.active_downloads > 0 {
        out.extend_from_slice(b" *");
    }

    out.push(b' ');
    if spec.show_nzbname != 0 {
        let mut nzb_nice = Vec::new();
        nzb_nice.extend_from_slice(unsafe { cstr_bytes(spec.nzb_name) });
        nzb_nice.push(spec.path_sep);
        nzb_nice = truncate_bstring_1024(nzb_nice);
        out.extend_from_slice(&nzb_nice);
    }
    out.extend_from_slice(unsafe { cstr_bytes(spec.filename) });
    out.extend_from_slice(b" (");
    crate::util::format_size_into(&mut out, spec.size);

    let completed_pct = crate::connclientmain::remote_client_calc_file_completed_pct(
        spec.remaining_size,
        spec.size,
    );
    if completed_pct >= 0 {
        out.extend_from_slice(b", ");
        out.extend_from_slice(completed_pct.to_string().as_bytes());
        out.push(b'%');
    }
    out.push(b')');
    if spec.paused != 0 {
        out.extend_from_slice(b" (paused)");
    }

    (truncate_bstring_1024(out), color)
}

/// FFI descriptor for `NCursesFrontend::PrintGroupname`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NcursesGroupRowSpec {
    pub edit_queue_mode: i32,
    pub selected: i32,
    pub use_color: i32,
    pub color_highl: i32,
    pub color_text: i32,
    pub screen_width: i32,
    pub calc_col_width: i32,
    pub id: i32,
    pub priority: i32,
    pub active_downloads: i32,
    pub name: *const u8,
    pub file_count: i32,
    pub paused_file_count: i32,
    pub size: i64,
    pub remaining_size: i64,
    pub paused_size: i64,
    pub stand_by: i32,
    pub pause_download: i32,
    pub current_download_speed: i64,
}

/// Format or measure a group row in `NCursesFrontend::PrintGroupname`.
pub fn format_group_row(
    spec: &NcursesGroupRowSpec,
    col_width_files: &mut i32,
    col_width_total: &mut i32,
    col_width_left: &mut i32,
) -> (Vec<u8>, i32) {
    let (color, brace1, brace2) = crate::schedfrontweb::ncurses_select_row_highlight(
        spec.edit_queue_mode != 0,
        spec.selected != 0,
        spec.use_color != 0,
        spec.color_highl,
        spec.color_text,
    );

    let name_len = crate::connclientmain::ncurses_calc_group_name_len(
        spec.screen_width,
        spec.calc_col_width != 0,
        *col_width_files,
        *col_width_total,
        *col_width_left,
    );

    let name_bytes = unsafe { cstr_bytes(spec.name) };
    if name_len > 20 {
        let mut files = Vec::with_capacity(24);
        files.extend_from_slice(spec.file_count.to_string().as_bytes());
        files.push(b'/');
        files.extend_from_slice(spec.paused_file_count.to_string().as_bytes());

        let mut name_with_ids = Vec::with_capacity(name_bytes.len() + 32);
        name_with_ids.push(brace1);
        name_with_ids.extend_from_slice(spec.id.to_string().as_bytes());
        name_with_ids.push(brace2);
        if spec.priority != 0 {
            name_with_ids.extend_from_slice(b" [");
            if spec.priority > 0 {
                name_with_ids.push(b'+');
            }
            name_with_ids.extend_from_slice(spec.priority.to_string().as_bytes());
            name_with_ids.push(b']');
        }
        if spec.active_downloads > 0 {
            name_with_ids.extend_from_slice(b" *");
        }
        name_with_ids.push(b' ');
        name_with_ids.extend_from_slice(name_bytes);
        name_with_ids = truncate_bstring_1024(name_with_ids);

        let unpaused_rem = spec.remaining_size.wrapping_sub(spec.paused_size);
        let mut remaining = Vec::with_capacity(24);
        let mut total = Vec::with_capacity(24);
        crate::util::format_size_into(&mut total, spec.size);

        let mut time = Vec::with_capacity(16);
        let cur_speed = if spec.stand_by != 0 {
            0
        } else {
            spec.current_download_speed
        };
        if spec.paused_size > 0 && unpaused_rem == 0 {
            time.extend_from_slice(b"[paused]");
            crate::util::format_size_into(&mut remaining, spec.remaining_size);
        } else {
            crate::util::format_size_into(&mut remaining, unpaused_rem);
            if let Some((h, m, s)) =
                crate::schedfrontweb::ncurses_calc_group_remain_hms_capped(
                    unpaused_rem,
                    cur_speed,
                    spec.pause_download != 0,
                )
            {
                push_hms(&mut time, h, m, s);
            }
        }

        if spec.calc_col_width != 0 {
            let wf = files.len() as i32;
            if wf > *col_width_files {
                *col_width_files = wf;
            }
            let wt = total.len() as i32;
            if wt > *col_width_total {
                *col_width_total = wt;
            }
            let wl = remaining.len() as i32;
            if wl > *col_width_left {
                *col_width_left = wl;
            }
            (Vec::new(), color)
        } else {
            let nl = name_len as usize;
            let mut out = Vec::with_capacity(spec.screen_width.max(64) as usize);
            let cut_len = name_with_ids.len().min(nl);
            out.extend_from_slice(&name_with_ids[..cut_len]);
            if cut_len < nl {
                out.resize(out.len() + (nl - cut_len), b' ');
            }
            out.extend_from_slice(b"  ");
            let push_right_aligned = |buf: &mut Vec<u8>, field: &[u8], width: usize| {
                if field.len() < width {
                    buf.resize(buf.len() + (width - field.len()), b' ');
                }
                buf.extend_from_slice(field);
            };
            push_right_aligned(&mut out, &files, (*col_width_files).max(0) as usize);
            out.extend_from_slice(b"  ");
            push_right_aligned(&mut out, &total, (*col_width_total).max(0) as usize);
            out.extend_from_slice(b"  ");
            push_right_aligned(&mut out, &remaining, (*col_width_left).max(0) as usize);
            out.extend_from_slice(b"  ");
            push_right_aligned(&mut out, &time, 8);
            (truncate_bstring_1024(out), color)
        }
    } else if spec.calc_col_width != 0 {
        (Vec::new(), color)
    } else {
        let mut out = Vec::with_capacity(name_bytes.len() + 24);
        out.push(brace1);
        out.extend_from_slice(spec.id.to_string().as_bytes());
        out.push(brace2);
        if spec.active_downloads > 0 {
            out.extend_from_slice(b" *");
        }
        out.push(b' ');
        out.extend_from_slice(name_bytes);
        (truncate_bstring_1024(out), color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_status_bar() {
        let spec = NcursesStatusSpec {
            stand_by: 0,
            pause_download: 0,
            current_download_speed: 1024 * 1024,
            remaining_size: 10 * 1024 * 1024,
            download_limit: 2048 * 1024,
            post_job_count: 2,
            dn_time_sec: 10,
            all_bytes: 5 * 1024 * 1024,
            thread_count: 8,
        };
        let line = String::from_utf8(format_status_bar(&spec)).unwrap();
        assert_eq!(
            line,
            " 8 threads, 1.00 MB/s, 10.00 MB remaining (~ 00:00:10), 2 post-jobs, Limit 2048 KB/s, Avg. 512 KB/s"
        );
    }

    #[test]
    fn test_format_top_header() {
        let hdr = format_top_header(b" Files for downloading", 80, true, 3661);
        let text = String::from_utf8(hdr).unwrap();
        assert_eq!(text.len(), 80);
        assert!(text.starts_with(" Files for downloading"));
        assert!(text.ends_with(" Uptime 01:01:01"));
    }

    #[test]
    fn test_format_file_and_group_rows() {
        let fspec = NcursesFileRowSpec {
            edit_queue_mode: 1,
            selected: 1,
            use_color: 0,
            color_highl: 8,
            color_text: 2,
            id: 42,
            priority: 10,
            active_downloads: 1,
            paused: 1,
            show_nzbname: 1,
            path_sep: b'/',
            nzb_name: c"MyNzb".as_ptr().cast(),
            filename: c"video.mkv".as_ptr().cast(),
            size: 200 * 1024 * 1024,
            remaining_size: 50 * 1024 * 1024,
        };
        let (frow, color) = format_file_row(&fspec);
        assert_eq!(color, 8);
        assert_eq!(
            String::from_utf8(frow).unwrap(),
            "<42> [+10] * MyNzb/video.mkv (200.00 MB, 75%) (paused)"
        );

        let mut wf = 0;
        let mut wt = 0;
        let mut wl = 0;
        let mut gspec = NcursesGroupRowSpec {
            edit_queue_mode: 0,
            selected: 0,
            use_color: 1,
            color_highl: 8,
            color_text: 2,
            screen_width: 80,
            calc_col_width: 1,
            id: 5,
            priority: 0,
            active_downloads: 2,
            name: c"Ubuntu.ISO".as_ptr().cast(),
            file_count: 10,
            paused_file_count: 2,
            size: 100 * 1024 * 1024,
            remaining_size: 50 * 1024 * 1024,
            paused_size: 10 * 1024 * 1024,
            stand_by: 0,
            pause_download: 0,
            current_download_speed: 1024 * 1024,
        };
        format_group_row(&gspec, &mut wf, &mut wt, &mut wl);
        assert_eq!((wf, wt, wl), (4, 9, 8));

        gspec.calc_col_width = 0;
        let (grow, gcolor) = format_group_row(&gspec, &mut wf, &mut wt, &mut wl);
        assert_eq!(gcolor, 2);
        let gtext = String::from_utf8(grow).unwrap();
        assert_eq!(gtext.len(), 80);
        assert!(gtext.starts_with("[5] * Ubuntu.ISO"));
        assert!(gtext.ends_with("10/2  100.00 MB  40.00 MB  00:00:40"));
    }
}
