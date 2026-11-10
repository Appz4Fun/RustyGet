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
        let (h, m, s) = crate::binrpc::remote_client_calc_hms(remain_sec);
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

    let pause_suffix = frontend_select_pause_status_suffix(
        spec.pause_download != 0,
        spec.stand_by != 0,
    );
    out.extend_from_slice(pause_suffix.to_bytes());

    if spec.download_limit > 0 {
        let limit_kb = frontend_calc_limit_kb(spec.download_limit as i32);
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

/// Format the colored console status bar (`ColoredFrontend::PrintStatus`).
#[allow(clippy::too_many_arguments)]
pub fn format_colored_status_bar(
    stand_by: i32,
    pause_download: i32,
    current_download_speed: i64,
    remaining_size: i64,
    download_limit: i64,
    post_job_count: i32,
    thread_count: i32,
    control_seq: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(160);
    let cur_speed = if stand_by != 0 {
        0
    } else {
        current_download_speed
    };

    out.push(b' ');
    out.extend_from_slice(thread_count.to_string().as_bytes());
    out.extend_from_slice(b" threads, ");
    out.extend_from_slice(&crate::util::format_speed(cur_speed));
    out.extend_from_slice(b", ");
    crate::util::format_size_into(&mut out, remaining_size);
    out.extend_from_slice(b" remaining");

    if let Some((h, m, s)) = frontend_calc_remaining_hms(
        remaining_size,
        current_download_speed,
        stand_by != 0,
        pause_download != 0,
    ) {
        out.extend_from_slice(b" (~ ");
        push_hms(&mut out, h, m, s);
        out.push(b')');
    }

    if post_job_count > 0 {
        out.extend_from_slice(b", ");
        out.extend_from_slice(post_job_count.to_string().as_bytes());
        out.extend_from_slice(if post_job_count > 1 {
            b" post-jobs"
        } else {
            b" post-job"
        });
    }

    let pause_suffix = frontend_select_pause_status_suffix(
        pause_download != 0,
        stand_by != 0,
    );
    out.extend_from_slice(pause_suffix.to_bytes());

    if download_limit > 0 {
        let limit_kb = frontend_calc_limit_kb(download_limit as i32);
        out.extend_from_slice(b", Limit ");
        out.extend_from_slice(limit_kb.to_string().as_bytes());
        out.extend_from_slice(b" KB/s");
    }

    out.extend_from_slice(control_seq);
    out.push(b'\n');

    truncate_bstring_1024(out)
}

/// Format a log message line for `ColoredFrontend::PrintMessage`, `NServFrontend::PrintMessage`,
/// or `LoggableFrontend::PrintMessage`.
/// `kind`: `0` = `mkInfo`, `1` = `mkWarning`, `2` = `mkError`, `3` = `mkDebug`, `4` = `mkDetail`.
pub fn format_log_message_line(colored: bool, kind: i32, text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 32);
    if colored {
        match kind {
            0 => out.extend_from_slice(b"\x1b[32m[INFO]\x1b[39m "),
            1 => out.extend_from_slice(b"\x1b[35m[WARNING]\x1b[39m "),
            2 => out.extend_from_slice(b"\x1b[31m[ERROR]\x1b[39m "),
            3 => out.extend_from_slice(b"[DEBUG] "),
            4 => out.extend_from_slice(b"\x1b[32m[DETAIL]\x1b[39m "),
            _ => return out,
        }
        out.extend_from_slice(text);
        out.extend_from_slice(b"\x1b[K\n");
    } else {
        match kind {
            0 => out.extend_from_slice(b"[INFO] "),
            1 => out.extend_from_slice(b"[WARNING] "),
            2 => out.extend_from_slice(b"[ERROR] "),
            3 => out.extend_from_slice(b"[DEBUG] "),
            4 => out.extend_from_slice(b"[DETAIL] "),
            _ => return out,
        }
        out.extend_from_slice(text);
        out.push(b'\n');
    }
    out
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

    let (d, h, m, s) = ncurses_calc_top_header_time(time_sec);
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

    let cap_mode = ncurses_select_top_header_caption(
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
    let (color, brace1, brace2) = ncurses_select_row_highlight(
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

    let completed_pct = crate::binrpc::remote_client_calc_file_completed_pct(
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
    let (color, brace1, brace2) = ncurses_select_row_highlight(
        spec.edit_queue_mode != 0,
        spec.selected != 0,
        spec.use_color != 0,
        spec.color_highl,
        spec.color_text,
    );

    let name_len = ncurses_calc_group_name_len(
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
                ncurses_calc_group_remain_hms_capped(
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

/// Computes `(hours, minutes, seconds)` for the remaining download time in
/// `ColoredFrontend::PrintStatus` and `NCursesFrontend::PrintStatus`.
pub fn frontend_calc_remaining_hms(
    remaining_size: i64,
    current_speed: i64,
    stand_by: bool,
    pause_download: bool,
) -> Option<(i32, i32, i32)> {
    let speed = if stand_by { 0 } else { current_speed };
    if speed > 0 && !pause_download {
        let remain_sec = remaining_size / speed;
        let h = (remain_sec / 3600) as i32;
        let m = ((remain_sec % 3600) / 60) as i32;
        let s = (remain_sec % 60) as i32;
        Some((h, m, s))
    } else {
        None
    }
}

/// Decomposes `time_sec` into `(days, hours, minutes, seconds)` for `NCursesFrontend::PrintTopHeader`.
pub fn ncurses_calc_top_header_time(time_sec: i32) -> (i32, i32, i32, i32) {
    let d = time_sec / 3600 / 24;
    let h = (time_sec % (3600 * 24)) / 3600;
    let m = (time_sec % 3600) / 60;
    let s = time_sec % 60;
    (d, h, m, s)
}

/// `ColoredFrontend::PrintStatus` / `NCursesFrontend::PrintStatus`: selects the pause status suffix string.
#[inline]
pub fn frontend_select_pause_status_suffix(
    pause_download: bool,
    stand_by: bool,
) -> &'static std::ffi::CStr {
    if pause_download {
        if stand_by {
            c", Paused"
        } else {
            c", Pausing"
        }
    } else {
        c""
    }
}

/// `ColoredFrontend::PrintStatus` / `NCursesFrontend::PrintStatus`: computes the speed limit in KB/s.
#[inline]
pub fn frontend_calc_limit_kb(download_limit: i32) -> i32 {
    if download_limit > 0 {
        download_limit / 1024
    } else {
        0
    }
}

/// `NCursesFrontend::PrintFilename` / `NCursesFrontend::PrintGroupname`: resolves `(color, brace1, brace2)`.
#[inline]
pub fn ncurses_select_row_highlight(
    is_edit_queue: bool,
    selected: bool,
    use_color: bool,
    highl_color: i32,
    text_color: i32,
) -> (i32, u8, u8) {
    if is_edit_queue && selected {
        if use_color {
            (highl_color, b'[', b']')
        } else {
            (highl_color, b'<', b'>')
        }
    } else {
        (text_color, b'[', b']')
    }
}

/// `NCursesFrontend::PrintTopHeader`: selects which right-aligned uptime/download-time caption fits in `chars_left`.
#[inline]
pub fn ncurses_select_top_header_caption(
    chars_left: i32,
    time_len: i32,
    long_cap_len: i32,
    short_cap_len: i32,
) -> i32 {
    if chars_left - time_len - long_cap_len >= 0 {
        2
    } else if chars_left - time_len - short_cap_len >= 0 {
        1
    } else if chars_left - time_len >= 0 {
        0
    } else {
        -1
    }
}

/// `NCursesFrontend::PrintGroupname`: computes `(h, m, s)` for a group's remaining download time.
#[inline]
pub fn ncurses_calc_group_remain_hms_capped(
    unpaused_remaining_size: i64,
    current_download_speed: i64,
    pause_download: bool,
) -> Option<(i32, i32, i32)> {
    if current_download_speed <= 0 || pause_download {
        return None;
    }
    let remain_sec = unpaused_remaining_size / current_download_speed;
    let h = (remain_sec / 3600) as i32;
    let m = ((remain_sec % 3600) / 60) as i32;
    let s = (remain_sec % 60) as i32;
    if h < 100 {
        Some((h, m, s))
    } else {
        Some((99, 99, 99))
    }
}

/// Computes `nameLen` in `NCursesFrontend::PrintGroupname`.
pub fn ncurses_calc_group_name_len(
    screen_width: i32,
    calc_col_width: bool,
    col_files: i32,
    col_total: i32,
    col_left: i32,
) -> i32 {
    if calc_col_width {
        screen_width - 1 - 9 - 11 - 11 - 9
    } else {
        screen_width - 1 - col_files - 2 - col_total - 2 - col_left - 2 - 9
    }
}

// ---- Pure-Rust POSIX Terminal / ANSI TUI Backend (replacing C ncurses) ----

struct TermState {
    saved_termios: Option<libc::termios>,
    frame_buf: Vec<u8>,
    input_buf: Vec<u8>,
    esc_started_at: Option<std::time::Instant>,
}

static TERM_STATE: std::sync::Mutex<TermState> = std::sync::Mutex::new(TermState {
    saved_termios: None,
    frame_buf: Vec::new(),
    input_buf: Vec::new(),
    esc_started_at: None,
});

/// Initializes the terminal for raw/non-blocking TUI mode and enters alternate screen.
/// Returns `1` if ANSI colors are supported, `0` if monochrome, or `-1` on error.
pub fn term_init() -> i32 {
    let mut guard = TERM_STATE.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        if libc::isatty(libc::STDIN_FILENO) != 0 {
            let mut orig: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut orig) == 0 {
                let mut raw = orig;
                raw.c_lflag &= !(libc::ICANON | libc::ECHO);
                raw.c_cc[libc::VMIN] = 0;
                raw.c_cc[libc::VTIME] = 0;
                let _ = libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);
                guard.saved_termios = Some(orig);
            }
        }
        let init_seq = b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H";
        let _ = libc::write(
            libc::STDOUT_FILENO,
            init_seq.as_ptr().cast(),
            init_seq.len(),
        );
    }
    guard.frame_buf.clear();
    guard.input_buf.clear();
    guard.esc_started_at = None;
    let term_env = std::env::var("TERM").unwrap_or_default();
    if term_env.is_empty() || term_env == "dumb" {
        0
    } else {
        1
    }
}

/// Restores terminal state and exits alternate screen.
pub fn term_shutdown() {
    let mut guard = TERM_STATE.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let exit_seq = b"\x1b[0m\x1b[?25h\x1b[?1049l";
        let _ = libc::write(
            libc::STDOUT_FILENO,
            exit_seq.as_ptr().cast(),
            exit_seq.len(),
        );
        if let Some(orig) = guard.saved_termios.take() {
            let _ = libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &orig);
        }
    }
    guard.frame_buf.clear();
    guard.input_buf.clear();
    guard.esc_started_at = None;
}

/// Queries terminal `(rows, cols)` via `TIOCGWINSZ`, defaulting to `(24, 80)`.
pub fn term_get_size() -> (i32, i32) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0
            && ws.ws_row > 0
            && ws.ws_col > 0
        {
            return (ws.ws_row as i32, ws.ws_col as i32);
        }
    }
    (24, 80)
}

/// Appends a screen clear sequence to the current frame buffer.
pub fn term_clear() {
    let mut guard = TERM_STATE.lock().unwrap_or_else(|e| e.into_inner());
    guard.frame_buf.extend_from_slice(b"\x1b[2J\x1b[H");
}

fn ansi_color_pair_escape(color_pair: i32) -> &'static [u8] {
    match color_pair {
        1 | 5 => b"\x1b[37;40m", // WHITE on BLACK
        2 | 6 => b"\x1b[32;40m", // GREEN on BLACK
        3 => b"\x1b[35;40m",     // MAGENTA on BLACK
        4 => b"\x1b[31;40m",     // RED on BLACK
        7 => b"\x1b[34;47m",     // BLUE on WHITE (status)
        8 | 9 => b"\x1b[37;44m", // WHITE on BLUE (keybar / infoline)
        10 => b"\x1b[30;46m",    // BLACK on CYAN (highlight)
        11 => b"\x1b[30;43m",    // BLACK on YELLOW (cursor)
        12 => b"\x1b[37;41m",    // WHITE on RED (hint)
        _ => b"\x1b[0m",
    }
}

/// Appends positioned, colored text to the frame buffer, sanitizing control bytes in `text`.
pub fn term_plot_text(text: &[u8], row: i32, col: i32, use_color: bool, color_pair: i32, blink: bool) {
    let mut guard = TERM_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let r = (row.max(0) + 1) as u32;
    let c = (col.max(0) + 1) as u32;
    guard.frame_buf.extend_from_slice(format!("\x1b[{r};{c}H").as_bytes());
    if use_color {
        guard.frame_buf.extend_from_slice(ansi_color_pair_escape(color_pair));
        if blink {
            guard.frame_buf.extend_from_slice(b"\x1b[5m");
        }
    }
    for &b in text {
        guard
            .frame_buf
            .push(if b < 0x20 || b == 0x7f { b' ' } else { b });
    }
    if use_color {
        guard.frame_buf.extend_from_slice(b"\x1b[0m");
    }
}

/// Positions the cursor and flushes the accumulated frame buffer to stdout.
pub fn term_refresh(row: i32, col: i32) {
    let mut guard = TERM_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let r = (row.max(0) + 1) as u32;
    let c = (col.max(0) + 1) as u32;
    guard.frame_buf.extend_from_slice(format!("\x1b[{r};{c}H").as_bytes());
    if !guard.frame_buf.is_empty() {
        unsafe {
            let _ = libc::write(
                libc::STDOUT_FILENO,
                guard.frame_buf.as_ptr().cast(),
                guard.frame_buf.len(),
            );
        }
        guard.frame_buf.clear();
    }
}

const TERM_ESC_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);

fn decode_next_term_key(
    input_buf: &mut Vec<u8>,
    esc_started_at: &mut Option<std::time::Instant>,
    now: std::time::Instant,
) -> i32 {
    if input_buf.is_empty() {
        *esc_started_at = None;
        return -1;
    }
    let b0 = input_buf[0];
    if b0 == 0x7f || b0 == 0x08 {
        input_buf.drain(..1);
        *esc_started_at = None;
        return 263; // KEY_BACKSPACE
    }
    if b0 == 0x1b {
        const SEQS: &[(&[u8], i32)] = &[
            (b"\x1b[B", 258),
            (b"\x1bOB", 258),
            (b"\x1b[A", 259),
            (b"\x1bOA", 259),
            (b"\x1b[H", 262),
            (b"\x1bOH", 262),
            (b"\x1b[1~", 262),
            (b"\x1b[7~", 262),
            (b"\x1b[F", 360),
            (b"\x1bOF", 360),
            (b"\x1b[4~", 360),
            (b"\x1b[8~", 360),
            (b"\x1b[5~", 339),
            (b"\x1b[6~", 338),
        ];
        for &(seq, code) in SEQS {
            if input_buf.starts_with(seq) {
                input_buf.drain(..seq.len());
                *esc_started_at = None;
                return code;
            }
        }
        // If input is a prefix of a known escape sequence, retain it across polls until timeout.
        if SEQS
            .iter()
            .any(|(seq, _)| seq.len() > input_buf.len() && seq.starts_with(input_buf))
        {
            let started = *esc_started_at.get_or_insert(now);
            if now.saturating_duration_since(started) < TERM_ESC_TIMEOUT {
                return -1;
            }
        }
        input_buf.drain(..1);
        *esc_started_at = None;
        return 27;
    }
    input_buf.drain(..1);
    *esc_started_at = None;
    b0 as i32
}

fn poll_read_fd_nonblocking(fd: libc::c_int, input_buf: &mut Vec<u8>) {
    unsafe {
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        if libc::poll(&mut pfd, 1, 0) > 0 && (pfd.revents & libc::POLLIN) != 0 {
            let mut b = [0u8; 64];
            let n = libc::read(fd, b.as_mut_ptr().cast(), b.len());
            if n > 0 {
                input_buf.extend_from_slice(&b[..n as usize]);
            }
        }
    }
}

/// Reads a single key or ANSI escape sequence from non-blocking stdin, retaining buffered input.
/// Returns `-1` when no key is available.
pub fn term_read_key() -> i32 {
    let mut guard = TERM_STATE.lock().unwrap_or_else(|e| e.into_inner());
    poll_read_fd_nonblocking(libc::STDIN_FILENO, &mut guard.input_buf);
    let state = &mut *guard;
    decode_next_term_key(
        &mut state.input_buf,
        &mut state.esc_started_at,
        std::time::Instant::now(),
    )
}

/// `NCursesFrontend::SetCurrentQueueEntry` (`NCursesFrontend.cpp`).
pub fn ncurses_set_current_queue_entry(
    mut entry: i32,
    queue_size: i32,
    mut scroll_offset: i32,
    client_height: i32,
) -> (i32, i32) {
    if entry < 0 {
        entry = 0;
    } else if entry > queue_size - 1 {
        entry = queue_size - 1;
    }

    if entry > scroll_offset + client_height || entry < scroll_offset - client_height {
        scroll_offset = entry - client_height / 2;
    } else if entry < scroll_offset {
        scroll_offset -= client_height;
    } else if entry >= scroll_offset + client_height {
        scroll_offset += client_height;
    }

    if scroll_offset > queue_size - client_height {
        scroll_offset = queue_size - client_height;
    }
    if scroll_offset < 0 {
        scroll_offset = 0;
    }

    (entry, scroll_offset)
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NcursesKeyState {
    pub input_mode: i32,
    pub show_nzbname: i32,
    pub show_timestamp: i32,
    pub group_files: i32,
    pub queue_window_percentage: i32,
    pub selected_queue_entry: i32,
    pub queue_scroll_offset: i32,
    pub queue_win_client_height: i32,
    pub input_number_index: i32,
    pub input_value: i32,
    pub exit_proc: i32,
    pub calc_window_sizes: i32,
    pub need_update_data: i32,
    pub toggle_pause: i32,
    pub set_delete_hint: i32,
    pub edit_action: i32,
    pub edit_offset: i32,
    pub has_select_entry: i32,
    pub select_entry: i32,
    pub has_set_download_rate: i32,
    pub download_rate_bytes: i32,
    pub stop_loop: i32,
}

/// `NCursesFrontend::UpdateInput` per-key transition (`NCursesFrontend.cpp`).
pub fn ncurses_handle_key(state: &mut NcursesKeyState, key: i32, queue_size: i32) {
    state.exit_proc = 0;
    state.calc_window_sizes = 0;
    state.need_update_data = 0;
    state.toggle_pause = 0;
    state.set_delete_hint = 0;
    state.edit_action = 0;
    state.edit_offset = 0;
    state.has_select_entry = 0;
    state.select_entry = 0;
    state.has_set_download_rate = 0;
    state.download_rate_bytes = 0;
    state.stop_loop = 0;

    if state.input_mode == 0 || state.input_mode == 1 {
        match key {
            x if x == b'q' as i32 => {
                state.exit_proc = 1;
            }
            x if x == b'z' as i32 => {
                state.show_nzbname = i32::from(state.show_nzbname == 0);
            }
            x if x == b'w' as i32 => {
                state.queue_window_percentage = if state.queue_window_percentage == 50 {
                    100
                } else if state.queue_window_percentage == 100 && state.input_mode != 1 {
                    0
                } else {
                    50
                };
                state.calc_window_sizes = 1;
                state.has_select_entry = 1;
                state.select_entry = state.selected_queue_entry;
            }
            x if x == b'g' as i32 => {
                state.group_files = i32::from(state.group_files == 0);
                state.has_select_entry = 1;
                state.select_entry = state.selected_queue_entry;
                state.need_update_data = 1;
            }
            _ => {}
        }
    }

    if state.input_mode == 0 {
        match key {
            x if x == b'p' as i32 => {
                state.toggle_pause = 1;
            }
            x if x == b'e' as i32 || x == 10 || x == 13 => {
                if queue_size > 0 {
                    state.input_mode = 1;
                    if state.queue_window_percentage == 0 {
                        state.queue_window_percentage = 50;
                    }
                    state.stop_loop = 1;
                    return;
                }
            }
            x if x == b'r' as i32 => {
                state.input_mode = 2;
                state.input_number_index = 0;
                state.input_value = 0;
                state.stop_loop = 1;
                return;
            }
            x if x == b't' as i32 => {
                state.show_timestamp = i32::from(state.show_timestamp == 0);
            }
            _ => {}
        }
    }

    if state.input_mode == 1 {
        match key {
            x if x == b'e' as i32 || x == 10 || x == 13 => {
                state.input_mode = 0;
                state.stop_loop = 1;
                return;
            }
            258 => {
                if state.selected_queue_entry < queue_size - 1 {
                    state.has_select_entry = 1;
                    state.select_entry = state.selected_queue_entry + 1;
                }
            }
            259 => {
                if state.selected_queue_entry > 0 {
                    state.has_select_entry = 1;
                    state.select_entry = state.selected_queue_entry - 1;
                }
            }
            339 => {
                if state.selected_queue_entry > 0 {
                    state.has_select_entry = 1;
                    if state.selected_queue_entry == state.queue_scroll_offset {
                        state.queue_scroll_offset -= state.queue_win_client_height;
                        state.select_entry =
                            state.selected_queue_entry - state.queue_win_client_height;
                    } else {
                        state.select_entry = state.queue_scroll_offset;
                    }
                }
            }
            338 => {
                if state.selected_queue_entry < queue_size - 1 {
                    let bottom = state.queue_scroll_offset + state.queue_win_client_height - 1;
                    state.has_select_entry = 1;
                    if state.selected_queue_entry == bottom {
                        state.queue_scroll_offset += state.queue_win_client_height;
                        state.select_entry =
                            state.selected_queue_entry + state.queue_win_client_height;
                    } else {
                        state.select_entry = bottom;
                    }
                }
            }
            262 => {
                state.has_select_entry = 1;
                state.select_entry = 0;
            }
            360 => {
                state.has_select_entry = 1;
                state.select_entry = if queue_size > 0 { queue_size - 1 } else { 0 };
            }
            x if x == b'p' as i32 => {
                state.edit_action = 4;
                state.edit_offset = 0;
            }
            x if x == b'd' as i32 => {
                state.set_delete_hint = 1;
            }
            x if x == b'D' as i32 => {
                state.edit_action = 6;
                state.edit_offset = 0;
                state.has_select_entry = 1;
                state.select_entry = state.selected_queue_entry;
            }
            x if x == b'u' as i32 => {
                state.edit_action = 1;
                state.edit_offset = -1;
                state.has_select_entry = 1;
                state.select_entry = state.selected_queue_entry - 1;
            }
            x if x == b'n' as i32 => {
                state.edit_action = 1;
                state.edit_offset = 1;
                state.has_select_entry = 1;
                state.select_entry = state.selected_queue_entry + 1;
            }
            x if x == b't' as i32 => {
                state.edit_action = 2;
                state.edit_offset = 0;
                state.has_select_entry = 1;
                state.select_entry = 0;
            }
            x if x == b'b' as i32 => {
                state.edit_action = 3;
                state.edit_offset = 0;
                state.has_select_entry = 1;
                state.select_entry = if queue_size > 0 { queue_size - 1 } else { 0 };
            }
            _ => {}
        }
    }

    if state.input_mode == 2 {
        if state.input_number_index < 5 && (b'0' as i32..=b'9' as i32).contains(&key) {
            state.input_value = (state.input_value * 10) + (key - b'0' as i32);
            state.input_number_index += 1;
        } else if key == 10 || key == 13 {
            state.has_set_download_rate = 1;
            state.download_rate_bytes = state.input_value * 1024;
            state.input_mode = 0;
            state.stop_loop = 1;
        } else if key == 27 {
            state.input_mode = 0;
            state.stop_loop = 1;
        } else if state.input_number_index > 0 && key == 263 {
            state.input_value = (state.input_value - (state.input_value % 10)) / 10;
            state.input_number_index -= 1;
        }
    }
}

/// `NCursesFrontend::PlotLine` (`NCursesFrontend.cpp`).
pub fn term_plot_line(
    text: &[u8],
    row: i32,
    pos: i32,
    screen_width: i32,
    use_color: bool,
    color_pair: i32,
) {
    let w = screen_width.clamp(0, MAX_SCREEN_WIDTH as i32) as usize;
    let mut line_buf = [b' '; MAX_SCREEN_WIDTH];
    let copy_len = text.len().min(w);
    if copy_len > 0 {
        line_buf[..copy_len].copy_from_slice(&text[..copy_len]);
    }
    let mut target_len = w;
    let rem = screen_width.wrapping_sub(pos);
    if (w as i32) + 1 > rem && rem < MAX_SCREEN_WIDTH as i32 {
        target_len = rem.clamp(0, w as i32) as usize;
    }
    term_plot_text(&line_buf[..target_len], row, pos, use_color, color_pair, false);
}

/// `NCursesFrontend::CalcWindowSizes` (`NCursesFrontend.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn ncurses_calc_window_sizes(
    queue_size: i32,
    queue_window_percentage: i32,
    screen_height: &mut i32,
    screen_width: &mut i32,
    queue_win_top: &mut i32,
    queue_win_height: &mut i32,
    queue_win_client_height: &mut i32,
    messages_win_top: &mut i32,
    messages_win_height: &mut i32,
    messages_win_client_height: &mut i32,
) {
    let (nr_rows, raw_cols) = term_get_size();
    let nr_cols = raw_cols.clamp(9, MAX_SCREEN_WIDTH as i32);
    if nr_rows != *screen_height || nr_cols != *screen_width {
        term_clear();
        *screen_height = nr_rows;
        *screen_width = nr_cols;
    }
    *queue_win_top = 0;
    *queue_win_height = (nr_rows - 2) * queue_window_percentage / 100;
    if *queue_win_height - 1 > queue_size {
        *queue_win_height = if queue_size > 0 { queue_size + 1 } else { 2 };
    }
    *queue_win_client_height = (*queue_win_height - 1).max(0);
    *messages_win_top = *queue_win_height;
    *messages_win_height = nr_rows - *queue_win_height - 2;
    *messages_win_client_height = (*messages_win_height - 1).max(0);
}

/// `NCursesFrontend::PrintMessage` (`NCursesFrontend.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn ncurses_print_message(
    msg_kind: i32,
    msg_time: i64,
    msg_text: &[u8],
    row: i32,
    max_lines: i32,
    show_timestamp: bool,
    time_correction: i32,
    screen_width: i32,
    use_color: bool,
) -> i32 {
    let mut text = if show_timestamp {
        let mut tbuf = [0u8; 50];
        crate::util::util_format_time(msg_time.wrapping_add(i64::from(time_correction)), &mut tbuf);
        let tlen = tbuf.iter().position(|&b| b == 0).unwrap_or(tbuf.len());
        let mut v = Vec::with_capacity(tlen + 3 + msg_text.len());
        v.extend_from_slice(&tbuf[..tlen]);
        v.extend_from_slice(b" - ");
        v.extend_from_slice(msg_text);
        v
    } else {
        msg_text.to_vec()
    };

    for b in &mut text {
        if *b == b'\n' || *b == b'\r' || *b == b'\x08' {
            *b = b' ';
        }
    }

    let len = text.len() as i32;
    let win_width = screen_width - 8;
    let msg_lines = if win_width > 0 {
        (len / win_width) + i32::from(len % win_width > 0)
    } else {
        0
    };

    const MSG_TYPES: [&[u8]; 5] = [
        b"INFO    ",
        b"WARNING ",
        b"ERROR   ",
        b"DEBUG   ",
        b"DETAIL  ",
    ];
    let color_pair = if (0..5).contains(&msg_kind) {
        msg_kind + 2
    } else {
        1
    };

    let mut lines = 0;
    let mut i = msg_lines - 1;
    while i >= 0 && lines < max_lines {
        let r = row - msg_lines + i + 1;
        let offset = (win_width * i).max(0) as usize;
        let slice = text.get(offset..).unwrap_or(b"");
        term_plot_line(slice, r, 8, screen_width, use_color, 1);
        let prefix = if i == 0 && (0..5).contains(&msg_kind) {
            MSG_TYPES[msg_kind as usize]
        } else {
            b"        "
        };
        term_plot_text(prefix, r, 0, use_color, color_pair, false);
        lines += 1;
        i -= 1;
    }

    lines
}

/// `NCursesFrontend::PrintKeyInputBar` (`NCursesFrontend.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn ncurses_print_key_input_bar(
    screen_height: i32,
    screen_width: i32,
    use_color: bool,
    input_mode: i32,
    group_files: bool,
    selected_queue_entry: i32,
    queue_size: i32,
    input_value: i32,
    input_number_index: i32,
    hint: &[u8],
    start_hint: &mut i64,
    cur_time: i64,
) {
    let input_bar_row = screen_height - 1;

    if !hint.is_empty() {
        if cur_time - *start_hint < 5 {
            term_plot_line(hint, input_bar_row, 0, screen_width, use_color, 12);
            return;
        }
        *start_hint = cur_time;
    }

    match input_mode {
        0 => {
            let line: &[u8] = if group_files {
                b"(Q)uit | (E)dit | (P)ause | (R)ate | (W)indow | (G)roup | (T)ime"
            } else {
                b"(Q)uit | (E)dit | (P)ause | (R)ate | (W)indow | (G)roup | (T)ime | n(Z)b"
            };
            term_plot_line(line, input_bar_row, 0, screen_width, use_color, 8);
        }
        1 => {
            let status: &[u8] = if selected_queue_entry > 0
                && queue_size > 1
                && selected_queue_entry == queue_size - 1
            {
                b"(Q)uit | (E)xit | (P)ause | (D)elete | (U)p/(T)op"
            } else if queue_size > 1 && selected_queue_entry == 0 {
                b"(Q)uit | (E)xit | (P)ause | (D)elete | dow(N)/(B)ottom"
            } else if queue_size > 1 {
                b"(Q)uit | (E)xit | (P)ause | (D)elete | (U)p/dow(N)/(T)op/(B)ottom"
            } else {
                b"(Q)uit | (E)xit | (P)ause | (D)elete"
            };
            term_plot_line(status, input_bar_row, 0, screen_width, use_color, 8);
        }
        2 => {
            let msg = format!("Download rate: {input_value}");
            term_plot_line(msg.as_bytes(), input_bar_row, 0, screen_width, use_color, 8);
            term_plot_text(b" ", input_bar_row, 15 + input_number_index, use_color, 11, true);
        }
        _ => {}
    }
}

/// `NCursesFrontend::EditQueue` group-mode action mapping (`NCursesFrontend.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn ncurses_resolve_group_edit_action(
    mut action: i32,
    has_group: bool,
    remaining_size: i64,
    paused_size: i64,
    remaining_par_count: i32,
    selected_entry: i32,
    last_edit_entry: i32,
    last_pause_pars: &mut bool,
) -> i32 {
    if has_group && action == 4 {
        if remaining_size == paused_size {
            action = 5;
        } else if paused_size == 0
            && remaining_par_count > 0
            && !(*last_pause_pars && last_edit_entry == selected_entry)
        {
            action = 8;
            *last_pause_pars = true;
        } else {
            *last_pause_pars = false;
        }
    }
    match action {
        1 => 11,
        2 => 12,
        3 => 13,
        4 => 16,
        5 => 17,
        6 => 18,
        7 => 22,
        8 => 23,
        _ => 0,
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

    #[test]
    fn test_format_colored_status_and_log_messages() {
        let bar = format_colored_status_bar(
            0,
            0,
            1024 * 1024,
            10 * 1024 * 1024,
            2048 * 1024,
            1,
            2,
            b"\x1b[K",
        );
        assert_eq!(
            String::from_utf8(bar).unwrap(),
            " 2 threads, 1.00 MB/s, 10.00 MB remaining (~ 00:00:10), 1 post-job, Limit 2048 KB/s\x1b[K\n"
        );

        let colored_err = format_log_message_line(true, 2, b"disk full");
        assert_eq!(colored_err, b"\x1b[31m[ERROR]\x1b[39m disk full\x1b[K\n");

        let plain_info = format_log_message_line(false, 0, b"started");
        assert_eq!(plain_info, b"[INFO] started\n");
    }

    #[test]
    fn test_decode_next_term_key_retains_incomplete_escape_sequences_across_polls() {
        let mut buf = Vec::new();
        let mut esc_started = None;
        let t0 = std::time::Instant::now();

        // Poll 1: only ESC arrives -> retained, returns -1
        buf.extend_from_slice(b"\x1b");
        assert_eq!(decode_next_term_key(&mut buf, &mut esc_started, t0), -1);
        assert_eq!(buf, b"\x1b");
        assert!(esc_started.is_some());

        // Poll 2 (10 ms later): '[' arrives -> still incomplete prefix `\x1b[`, returns -1
        buf.extend_from_slice(b"[");
        assert_eq!(
            decode_next_term_key(
                &mut buf,
                &mut esc_started,
                t0 + std::time::Duration::from_millis(10)
            ),
            -1
        );
        assert_eq!(buf, b"\x1b[");

        // Poll 3 (20 ms later): 'A' arrives -> complete KEY_UP (259)
        buf.extend_from_slice(b"A");
        assert_eq!(
            decode_next_term_key(
                &mut buf,
                &mut esc_started,
                t0 + std::time::Duration::from_millis(20)
            ),
            259
        );
        assert!(buf.is_empty());
        assert!(esc_started.is_none());

        // Bare ESC: retained before timeout, emitted as 27 once timeout elapses
        buf.extend_from_slice(b"\x1b");
        assert_eq!(decode_next_term_key(&mut buf, &mut esc_started, t0), -1);
        assert_eq!(
            decode_next_term_key(
                &mut buf,
                &mut esc_started,
                t0 + TERM_ESC_TIMEOUT + std::time::Duration::from_millis(1)
            ),
            27
        );
        assert!(buf.is_empty());
        assert!(esc_started.is_none());
    }

    #[test]
    fn test_poll_read_fd_nonblocking_on_idle_pipe() {
        let mut fds = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let mut buf = Vec::new();
        // Idle open pipe must return immediately without blocking
        poll_read_fd_nonblocking(fds[0], &mut buf);
        assert!(buf.is_empty());

        // Write a byte to the pipe and verify it is read
        let msg = b"q";
        assert_eq!(
            unsafe { libc::write(fds[1], msg.as_ptr().cast(), msg.len()) },
            1
        );
        poll_read_fd_nonblocking(fds[0], &mut buf);
        assert_eq!(buf, b"q");

        unsafe {
            let _ = libc::close(fds[0]);
            let _ = libc::close(fds[1]);
        }
    }

    #[test]
    fn test_ncurses_queue_selection_and_group_edit_action() {
        assert_eq!(ncurses_set_current_queue_entry(-5, 10, 0, 4), (0, 0));
        assert_eq!(ncurses_set_current_queue_entry(15, 10, 0, 4), (9, 6));

        let mut pause_pars = false;
        assert_eq!(
            ncurses_resolve_group_edit_action(4, true, 100, 0, 2, 0, -1, &mut pause_pars),
            23
        );
        assert!(pause_pars);
        assert_eq!(
            ncurses_resolve_group_edit_action(4, true, 100, 0, 2, 0, 0, &mut pause_pars),
            16
        );
        assert!(!pause_pars);
        assert_eq!(
            ncurses_resolve_group_edit_action(4, true, 100, 100, 0, 0, 0, &mut pause_pars),
            17
        );
    }
}
