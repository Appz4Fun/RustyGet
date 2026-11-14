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

    let pause_suffix =
        frontend_select_pause_status_suffix(spec.pause_download != 0, spec.stand_by != 0);
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

    let pause_suffix = frontend_select_pause_status_suffix(pause_download != 0, stand_by != 0);
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
pub fn format_top_header(
    header: &[u8],
    screen_width: i32,
    up_time: bool,
    time_sec: i32,
) -> Vec<u8> {
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

    let cap_mode =
        ncurses_select_top_header_caption(chars_left, time_len, long_cap_len, short_cap_len);

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

    let completed_pct =
        crate::binrpc::remote_client_calc_file_completed_pct(spec.remaining_size, spec.size);
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
            if let Some((h, m, s)) = ncurses_calc_group_remain_hms_capped(
                unpaused_rem,
                cur_speed,
                spec.pause_download != 0,
            ) {
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
pub fn term_plot_text(
    text: &[u8],
    row: i32,
    col: i32,
    use_color: bool,
    color_pair: i32,
    blink: bool,
) {
    let mut guard = TERM_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let r = (row.max(0) + 1) as u32;
    let c = (col.max(0) + 1) as u32;
    guard
        .frame_buf
        .extend_from_slice(format!("\x1b[{r};{c}H").as_bytes());
    if use_color {
        guard
            .frame_buf
            .extend_from_slice(ansi_color_pair_escape(color_pair));
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
    guard
        .frame_buf
        .extend_from_slice(format!("\x1b[{r};{c}H").as_bytes());
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
    term_plot_text(
        &line_buf[..target_len],
        row,
        pos,
        use_color,
        color_pair,
        false,
    );
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
            term_plot_text(
                b" ",
                input_bar_row,
                15 + input_number_index,
                use_color,
                11,
                true,
            );
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

// ---- Frontend Execution Engine (`Frontend`, `LoggableFrontend`, `NCursesFrontend`, `NServFrontend`) ----

pub type FrontendWriteStdoutFn =
    unsafe extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_char, usize, std::ffi::c_int);
pub type FrontendActionFn = unsafe extern "C" fn(
    *mut std::ffi::c_void,
    std::ffi::c_int,
    std::ffi::c_int,
    std::ffi::c_int,
    std::ffi::c_int,
) -> i64;
pub type FrontendLocalSummaryFn =
    unsafe extern "C" fn(*mut std::ffi::c_void, *mut crate::binrpc::FrontendSummary);
pub type FrontendEmitMsgFn =
    unsafe extern "C" fn(*mut std::ffi::c_void, u32, std::ffi::c_int, i64, *const std::ffi::c_char);
pub type FrontendLocalMessagesFn = unsafe extern "C" fn(
    *mut std::ffi::c_void,
    std::ffi::c_int,
    u32,
    *mut std::ffi::c_int,
    *mut std::ffi::c_void,
    FrontendEmitMsgFn,
);
pub type FrontendEmitNzbFn = unsafe extern "C" fn(
    *mut std::ffi::c_void,
    std::ffi::c_int,
    std::ffi::c_int,
    std::ffi::c_int,
    i64,
    i64,
    i64,
    std::ffi::c_int,
    std::ffi::c_int,
    *const std::ffi::c_char,
);
pub type FrontendEmitFileFn = unsafe extern "C" fn(
    *mut std::ffi::c_void,
    usize,
    std::ffi::c_int,
    std::ffi::c_int,
    std::ffi::c_int,
    i64,
    i64,
    *const std::ffi::c_char,
);
pub type FrontendLocalQueueFn = unsafe extern "C" fn(
    *mut std::ffi::c_void,
    *mut std::ffi::c_void,
    FrontendEmitNzbFn,
    FrontendEmitFileFn,
);

/// FFI environment for `nzbget_rs_frontend_op`.
#[repr(C)]
pub struct NzbgetRsFrontendEnv {
    pub ctx: *mut std::ffi::c_void,
    pub remote_mode: std::ffi::c_int,
    pub update_interval: std::ffi::c_int,
    pub use_color: std::ffi::c_int,
    pub show_nzbname: std::ffi::c_int,
    pub show_timestamp: std::ffi::c_int,
    pub group_files: std::ffi::c_int,
    pub time_correction: std::ffi::c_int,
    pub path_sep: u8,
    pub control_ip: *const std::ffi::c_char,
    pub control_port: std::ffi::c_int,
    pub control_username: *const std::ffi::c_char,
    pub control_password: *const std::ffi::c_char,
    pub write_stdout_cb: Option<FrontendWriteStdoutFn>,
    pub action_cb: Option<FrontendActionFn>,
    pub local_summary_cb: Option<FrontendLocalSummaryFn>,
    pub local_messages_cb: Option<FrontendLocalMessagesFn>,
    pub local_queue_cb: Option<FrontendLocalQueueFn>,
}

struct FrontendLogEntry {
    id: u32,
    kind: i32,
    time: i64,
    text: Vec<u8>,
}

struct FrontendFileEntry {
    id: i32,
    active_downloads: i32,
    paused: bool,
    size: i64,
    remaining_size: i64,
    filename_cstr: Vec<u8>,
}

struct FrontendGroupEntry {
    id: i32,
    priority: i32,
    active_downloads: i32,
    name_cstr: Vec<u8>,
    size: i64,
    remaining_size: i64,
    paused_size: i64,
    paused_file_count: i32,
    remaining_par_count: i32,
    files: Vec<FrontendFileEntry>,
}

unsafe fn cstr_with_nul(ptr: *const u8) -> Vec<u8> {
    if ptr.is_null() {
        vec![0]
    } else {
        std::ffi::CStr::from_ptr(ptr.cast())
            .to_bytes_with_nul()
            .to_vec()
    }
}

fn fe_write_stdout(env: &NzbgetRsFrontendEnv, bytes: &[u8], flush: bool) {
    if let Some(cb) = env.write_stdout_cb {
        unsafe {
            cb(
                env.ctx,
                if bytes.is_empty() {
                    std::ptr::null()
                } else {
                    bytes.as_ptr().cast()
                },
                bytes.len(),
                std::ffi::c_int::from(flush),
            );
        }
    }
}

fn fe_action(env: &NzbgetRsFrontendEnv, op: i32, a1: i32, a2: i32, a3: i32) -> i64 {
    if let Some(cb) = env.action_cb {
        unsafe { cb(env.ctx, op, a1, a2, a3) }
    } else {
        0
    }
}

fn fe_is_stopped(env: &NzbgetRsFrontendEnv) -> bool {
    fe_action(env, 0, 0, 0, 0) != 0
}

fn fe_collect_local_messages(
    env: &NzbgetRsFrontendEnv,
    tail_mode: i32,
    arg: u32,
) -> (bool, Vec<FrontendLogEntry>) {
    unsafe extern "C" fn push_msg(
        raw: *mut std::ffi::c_void,
        id: u32,
        kind: std::ffi::c_int,
        time: i64,
        text: *const std::ffi::c_char,
    ) {
        let out = &mut *raw.cast::<Vec<FrontendLogEntry>>();
        out.push(FrontendLogEntry {
            id,
            kind,
            time,
            text: cstr_bytes(text.cast()).to_vec(),
        });
    }
    let mut skipped: std::ffi::c_int = 0;
    let mut out = Vec::new();
    if let Some(cb) = env.local_messages_cb {
        unsafe {
            cb(
                env.ctx,
                tail_mode,
                arg,
                &mut skipped,
                (&mut out as *mut Vec<FrontendLogEntry>).cast(),
                push_msg,
            );
        }
    }
    (skipped != 0, out)
}

fn fe_collect_local_queue(env: &NzbgetRsFrontendEnv) -> Vec<FrontendGroupEntry> {
    unsafe extern "C" fn push_nzb(
        raw: *mut std::ffi::c_void,
        id: std::ffi::c_int,
        prio: std::ffi::c_int,
        act: std::ffi::c_int,
        sz: i64,
        rem: i64,
        psd: i64,
        pfc: std::ffi::c_int,
        rpc: std::ffi::c_int,
        name: *const std::ffi::c_char,
    ) {
        let groups = &mut *raw.cast::<Vec<FrontendGroupEntry>>();
        groups.push(FrontendGroupEntry {
            id,
            priority: prio,
            active_downloads: act,
            name_cstr: cstr_with_nul(name.cast()),
            size: sz,
            remaining_size: rem,
            paused_size: psd,
            paused_file_count: pfc,
            remaining_par_count: rpc,
            files: Vec::new(),
        });
    }
    unsafe extern "C" fn push_file(
        raw: *mut std::ffi::c_void,
        idx: usize,
        id: std::ffi::c_int,
        act: std::ffi::c_int,
        paused: std::ffi::c_int,
        sz: i64,
        rem: i64,
        fname: *const std::ffi::c_char,
    ) {
        let groups = &mut *raw.cast::<Vec<FrontendGroupEntry>>();
        if let Some(g) = groups.get_mut(idx) {
            g.files.push(FrontendFileEntry {
                id,
                active_downloads: act,
                paused: paused != 0,
                size: sz,
                remaining_size: rem,
                filename_cstr: cstr_with_nul(fname.cast()),
            });
        }
    }
    let mut groups = Vec::new();
    if let Some(cb) = env.local_queue_cb {
        unsafe {
            cb(
                env.ctx,
                (&mut groups as *mut Vec<FrontendGroupEntry>).cast(),
                push_nzb,
                push_file,
            );
        }
    }
    groups
}

#[cfg(unix)]
fn fe_make_remote_params<'a>(
    env: &'a NzbgetRsFrontendEnv,
    ids: &'a [i32],
) -> crate::binrpc::BinRpcClientParams<'a> {
    crate::binrpc::BinRpcClientParams {
        control_ip: unsafe { cstr_bytes(env.control_ip.cast()) },
        control_port: env.control_port,
        control_username: unsafe { cstr_bytes(env.control_username.cast()) },
        control_password: unsafe { cstr_bytes(env.control_password.cast()) },
        verbose: false,
        match_mode: 1,
        set_rate: 0,
        edit_action: 0,
        edit_offset: 0,
        edit_text: None,
        edit_ids: ids,
        edit_names: &[],
        log_lines: 0,
        log_first_id: 0,
        add_nzb_filename: b"",
        arg_filename: b"",
        add_category: b"",
        auto_category: false,
        add_top: false,
        add_paused: false,
        add_priority: 0,
        add_dupe_key: b"",
        add_dupe_mode: 0,
        add_dupe_score: 0,
        write_log_kind: 0,
        write_log_text: b"",
        want_file_list: false,
        want_summary: false,
    }
}

#[cfg(unix)]
fn fe_prepare_remote(
    env: &NzbgetRsFrontendEnv,
    want_summary: bool,
    want_file_list: bool,
    needed_log_entries: u32,
    needed_log_first_id: u32,
    summary: &mut crate::binrpc::FrontendSummary,
    messages: &mut Vec<FrontendLogEntry>,
    groups: &mut Vec<FrontendGroupEntry>,
) -> bool {
    if fe_is_stopped(env) {
        return false;
    }
    messages.clear();
    groups.clear();

    let mut params = fe_make_remote_params(env, &[]);
    params.log_lines = needed_log_entries as i32;
    params.log_first_id = needed_log_first_id;
    params.want_file_list = want_file_list;
    params.want_summary = want_summary;

    let mut ok = crate::binrpc::client_execute(
        100,
        &params,
        None,
        |_| {},
        |_| {},
        || fe_is_stopped(env),
        |id, kind, time, txt| {
            messages.push(FrontendLogEntry {
                id,
                kind: kind as i32,
                time: i64::from(time),
                text: unsafe { cstr_bytes(txt) }.to_vec(),
            });
        },
        |_, _, _, _, _, _, _, _, _, _, _, _, _| {},
        |_, _, _| {},
        |_, _, _, _, _, _, _, _, _| {},
    );

    if ok && (want_summary || want_file_list) {
        let mut raw_files: Vec<(usize, FrontendFileEntry)> = Vec::new();
        ok = crate::binrpc::client_execute(
            101,
            &params,
            want_summary.then_some(summary),
            |_| {},
            |_| {},
            || fe_is_stopped(env),
            |_, _, _, _| {},
            |id, _kind, sz, rem, psd, pfc, rpc, prio, _f0, nm, _f2, _f3, _f4| {
                groups.push(FrontendGroupEntry {
                    id: id as i32,
                    priority: prio,
                    active_downloads: 0,
                    name_cstr: unsafe { cstr_with_nul(nm) },
                    size: sz,
                    remaining_size: rem,
                    paused_size: psd,
                    paused_file_count: pfc,
                    remaining_par_count: rpc,
                    files: Vec::new(),
                });
            },
            |_, _, _| {},
            |idx, fid, sz, rem, psd, _fnc, act, _subj, fname| {
                raw_files.push((
                    idx,
                    FrontendFileEntry {
                        id: fid as i32,
                        active_downloads: act,
                        paused: psd != 0,
                        size: sz,
                        remaining_size: rem,
                        filename_cstr: unsafe { cstr_with_nul(fname) },
                    },
                ));
            },
        );
        for (idx, file_entry) in raw_files {
            if let Some(g) = groups.get_mut(idx) {
                g.active_downloads += file_entry.active_downloads;
                g.files.push(file_entry);
            }
        }
    }

    if !ok {
        let ctrl_ip = unsafe { cstr_bytes(env.control_ip.cast()) };
        let effective_ip: &[u8] = if ctrl_ip == b"0.0.0.0" {
            b"127.0.0.1"
        } else {
            ctrl_ip
        };
        let mut msg = Vec::with_capacity(68 + effective_ip.len());
        msg.extend_from_slice(b"\nUnable to send request to RustyGet server at ");
        msg.extend_from_slice(effective_ip);
        msg.extend_from_slice(b" (port ");
        msg.extend_from_slice(env.control_port.to_string().as_bytes());
        msg.extend_from_slice(b")    \n");
        fe_write_stdout(env, &msg, false);
        let _ = fe_action(env, 2, 0, 0, 0);
        messages.clear();
        groups.clear();
        return false;
    }
    true
}

#[cfg(not(unix))]
fn fe_prepare_remote(
    _env: &NzbgetRsFrontendEnv,
    _want_summary: bool,
    _want_file_list: bool,
    _needed_log_entries: u32,
    _needed_log_first_id: u32,
    _summary: &mut crate::binrpc::FrontendSummary,
    _messages: &mut Vec<FrontendLogEntry>,
    _groups: &mut Vec<FrontendGroupEntry>,
) -> bool {
    false
}

fn fe_server_pause_unpause(env: &NzbgetRsFrontendEnv, pause: bool, log_local_info: bool) {
    if env.remote_mode != 0 {
        #[cfg(unix)]
        {
            let params = fe_make_remote_params(env, &[]);
            let _ = crate::binrpc::client_execute(
                if pause { 16 } else { 17 },
                &params,
                None,
                |_| {},
                |_| {},
                || fe_is_stopped(env),
                |_, _, _, _| {},
                |_, _, _, _, _, _, _, _, _, _, _, _, _| {},
                |_, _, _| {},
                |_, _, _, _, _, _, _, _, _| {},
            );
        }
    } else {
        let _ = fe_action(env, 3, i32::from(pause), i32::from(log_local_info), 0);
    }
}

fn fe_server_set_rate(env: &NzbgetRsFrontendEnv, rate: i32) {
    if env.remote_mode != 0 {
        #[cfg(unix)]
        {
            let mut params = fe_make_remote_params(env, &[]);
            params.set_rate = rate;
            let _ = crate::binrpc::client_execute(
                5,
                &params,
                None,
                |_| {},
                |_| {},
                || fe_is_stopped(env),
                |_, _, _, _| {},
                |_, _, _, _, _, _, _, _, _, _, _, _, _| {},
                |_, _, _| {},
                |_, _, _, _, _, _, _, _, _| {},
            );
        }
    } else {
        let _ = fe_action(env, 4, rate, 0, 0);
    }
}

fn fe_server_edit_queue(env: &NzbgetRsFrontendEnv, action: i32, offset: i32, id: i32) -> bool {
    if env.remote_mode != 0 {
        #[cfg(unix)]
        {
            let ids = [id];
            let mut params = fe_make_remote_params(env, &ids);
            params.edit_action = action;
            params.edit_offset = offset;
            return crate::binrpc::client_execute(
                7,
                &params,
                None,
                |_| {},
                |_| {},
                || fe_is_stopped(env),
                |_, _, _, _| {},
                |_, _, _, _, _, _, _, _, _, _, _, _, _| {},
                |_, _, _| {},
                |_, _, _, _, _, _, _, _, _| {},
            );
        }
        #[cfg(not(unix))]
        {
            false
        }
    } else {
        fe_action(env, 5, action, offset, id) != 0
    }
}

/// Runs `LoggableFrontend` (`mode = 0` plain, `mode = 1` colored) or `NServFrontend` (`mode = 3`).
fn run_loggable(mode: i32, env: &NzbgetRsFrontendEnv) {
    let colored = mode == 1 || mode == 3;
    let want_summary = mode == 1;
    let mut need_go_back = false;
    let mut needed_log_first_id: u32 = 0;
    let mut summary = crate::binrpc::FrontendSummary::default();
    let mut remote_messages = Vec::new();
    let mut remote_groups = Vec::new();

    let mut update = |need_go_back: &mut bool,
                      needed_log_first_id: &mut u32,
                      summary: &mut crate::binrpc::FrontendSummary| {
        if env.remote_mode != 0 {
            if !fe_prepare_remote(
                env,
                want_summary,
                false,
                0,
                *needed_log_first_id,
                summary,
                &mut remote_messages,
                &mut remote_groups,
            ) {
                return;
            }
        } else if want_summary {
            if let Some(cb) = env.local_summary_cb {
                unsafe { cb(env.ctx, summary) };
            }
        }

        if mode == 1 && *need_go_back {
            fe_write_stdout(env, b"\r\x1b[1A", false);
            *need_go_back = false;
        }

        if env.remote_mode != 0 {
            if !remote_messages.is_empty() {
                let first_id = remote_messages[0].id;
                let mut start = needed_log_first_id.wrapping_sub(first_id).wrapping_add(1) as i32;
                if start < 0 {
                    fe_write_stdout(
                        env,
                        if colored {
                            b".....\x1b[K\n"
                        } else {
                            b".....\n"
                        },
                        false,
                    );
                    start = 0;
                }
                for m in remote_messages.iter().skip(start as usize) {
                    let line = format_log_message_line(colored, m.kind, &m.text);
                    fe_write_stdout(env, &line, false);
                    *needed_log_first_id = m.id;
                }
            }
            remote_messages.clear();
            remote_groups.clear();
        } else {
            let (skipped, msgs) = fe_collect_local_messages(env, 0, *needed_log_first_id);
            if skipped {
                fe_write_stdout(
                    env,
                    if colored {
                        b".....\x1b[K\n"
                    } else {
                        b".....\n"
                    },
                    false,
                );
            }
            for m in &msgs {
                let line = format_log_message_line(colored, m.kind, &m.text);
                fe_write_stdout(env, &line, false);
                *needed_log_first_id = m.id;
            }
        }

        if mode == 1 {
            fe_write_stdout(env, b"\x1b[s", false);
            let status = format_colored_status_bar(
                summary.stand_by,
                summary.pause_download,
                summary.current_download_speed,
                summary.remaining_size,
                i64::from(summary.download_limit),
                summary.post_job_count,
                summary.thread_count,
                b"\x1b[K",
            );
            fe_write_stdout(env, &status, false);
            *need_go_back = true;
        }

        fe_write_stdout(env, b"", true);
    };

    while !fe_is_stopped(env) {
        update(&mut need_go_back, &mut needed_log_first_id, &mut summary);
        let _ = fe_action(env, 1, env.update_interval, 0, 0);
    }
    update(&mut need_go_back, &mut needed_log_first_id, &mut summary);

    if mode == 1 && env.remote_mode != 0 {
        fe_write_stdout(env, b"\n", false);
    }
}

fn fe_calc_queue_size(groups: &[FrontendGroupEntry], group_files: bool) -> i32 {
    if group_files {
        groups.len() as i32
    } else {
        groups.iter().map(|g| g.files.len() as i32).sum()
    }
}

/// Runs `NCursesFrontend` (`mode = 2`).
fn run_ncurses(
    env: &NzbgetRsFrontendEnv,
    mut read_key: impl FnMut() -> i32,
    mut refresh_screen: impl FnMut(i32, i32),
) {
    const COLOR_TEXT: i32 = 1;
    const COLOR_STATUS: i32 = 7;
    const COLOR_INFOLINE: i32 = 9;
    const COLOR_TEXTHIGHL: i32 = 10;

    let use_color = env.use_color != 0;
    let mut data_update_pos: i32 = 0;
    let mut update_next_time = false;
    let mut screen_height: i32 = 0;
    let mut screen_width: i32 = 0;
    let mut queue_win_top: i32 = 0;
    let mut queue_win_height: i32 = 0;
    let mut queue_win_client_height: i32 = 0;
    let mut messages_win_top: i32 = 0;
    let mut messages_win_height: i32 = 0;
    let mut messages_win_client_height: i32 = 0;
    let mut selected_queue_entry: i32 = 0;
    let mut last_edit_entry: i32 = -1;
    let mut last_pause_pars = false;
    let mut queue_scroll_offset: i32 = 0;
    let mut hint: Vec<u8> = Vec::new();
    let mut start_hint: i64 = 0;
    let mut input_number_index: i32 = 0;
    let mut input_value: i32 = 0;
    let mut input_mode: i32 = 0;
    let mut show_nzbname = env.show_nzbname != 0;
    let mut show_timestamp = env.show_timestamp != 0;
    let mut group_files = env.group_files != 0;
    let mut queue_window_percentage: i32 = 50;

    let mut summary = crate::binrpc::FrontendSummary::default();
    let mut groups: Vec<FrontendGroupEntry> = Vec::new();
    let mut messages: Vec<FrontendLogEntry> = Vec::new();

    while !fe_is_stopped(env) {
        let mut update_now = false;
        let key = read_key();

        if key != -1 {
            update_now = true;
            update_next_time = true;
        } else if update_next_time {
            update_now = true;
            update_next_time = false;
        } else if data_update_pos <= 0 {
            update_now = true;
            update_next_time = false;
        }

        if update_now {
            'update_frame: {
                if env.remote_mode == 0 {
                    groups = fe_collect_local_queue(env);
                    messages = fe_collect_local_messages(env, 1, 0).1;
                }

                ncurses_calc_window_sizes(
                    fe_calc_queue_size(&groups, group_files),
                    queue_window_percentage,
                    &mut screen_height,
                    &mut screen_width,
                    &mut queue_win_top,
                    &mut queue_win_height,
                    &mut queue_win_client_height,
                    &mut messages_win_top,
                    &mut messages_win_height,
                    &mut messages_win_client_height,
                );

                if data_update_pos <= 0 {
                    if env.remote_mode != 0 {
                        let needed_entries = messages_win_client_height.max(0) as u32;
                        if !fe_prepare_remote(
                            env,
                            true,
                            true,
                            needed_entries,
                            0,
                            &mut summary,
                            &mut messages,
                            &mut groups,
                        ) {
                            break 'update_frame;
                        }
                    } else if let Some(cb) = env.local_summary_cb {
                        unsafe { cb(env.ctx, &mut summary) };
                    }

                    ncurses_calc_window_sizes(
                        fe_calc_queue_size(&groups, group_files),
                        queue_window_percentage,
                        &mut screen_height,
                        &mut screen_width,
                        &mut queue_win_top,
                        &mut queue_win_height,
                        &mut queue_win_client_height,
                        &mut messages_win_top,
                        &mut messages_win_height,
                        &mut messages_win_client_height,
                    );
                }

                if input_mode == 1 && fe_calc_queue_size(&groups, group_files) == 0 {
                    selected_queue_entry = 0;
                    input_mode = 0;
                }

                if queue_win_height > 0 {
                    let in_scroll = |idx: i32| {
                        idx >= queue_scroll_offset
                            && idx < queue_scroll_offset + queue_win_height - 1
                    };
                    if group_files {
                        let mut line_nr = queue_win_top;
                        if groups.is_empty() {
                            let hdr = format_group_queue_header(use_color, 0, 0, 0);
                            let top =
                                format_top_header(&hdr, screen_width, false, summary.dn_time_sec);
                            term_plot_line(
                                &top,
                                line_nr,
                                0,
                                screen_width,
                                use_color,
                                COLOR_INFOLINE,
                            );
                            line_nr += 1;
                            term_plot_line(
                                b"Ready to receive nzb-job",
                                line_nr,
                                0,
                                screen_width,
                                use_color,
                                COLOR_TEXT,
                            );
                        } else {
                            line_nr += 1;
                            let mut col_width_files: i32 = 0;
                            let mut col_width_total: i32 = 0;
                            let mut col_width_left: i32 = 0;
                            for (i, g) in groups.iter().enumerate() {
                                if in_scroll(i as i32) {
                                    let spec = NcursesGroupRowSpec {
                                        edit_queue_mode: i32::from(input_mode == 1),
                                        selected: 0,
                                        use_color: i32::from(use_color),
                                        color_highl: COLOR_TEXTHIGHL,
                                        color_text: COLOR_TEXT,
                                        screen_width,
                                        calc_col_width: 1,
                                        id: g.id,
                                        priority: g.priority,
                                        active_downloads: g.active_downloads,
                                        name: g.name_cstr.as_ptr(),
                                        file_count: g.files.len() as i32,
                                        paused_file_count: g.paused_file_count,
                                        size: g.size,
                                        remaining_size: g.remaining_size,
                                        paused_size: g.paused_size,
                                        stand_by: summary.stand_by,
                                        pause_download: summary.pause_download,
                                        current_download_speed: summary.current_download_speed,
                                    };
                                    let _ = format_group_row(
                                        &spec,
                                        &mut col_width_files,
                                        &mut col_width_total,
                                        &mut col_width_left,
                                    );
                                }
                            }

                            let mut remaining: i64 = 0;
                            let mut paused: i64 = 0;
                            for (i, g) in groups.iter().enumerate() {
                                if in_scroll(i as i32) {
                                    let spec = NcursesGroupRowSpec {
                                        edit_queue_mode: i32::from(input_mode == 1),
                                        selected: i32::from((i as i32) == selected_queue_entry),
                                        use_color: i32::from(use_color),
                                        color_highl: COLOR_TEXTHIGHL,
                                        color_text: COLOR_TEXT,
                                        screen_width,
                                        calc_col_width: 0,
                                        id: g.id,
                                        priority: g.priority,
                                        active_downloads: g.active_downloads,
                                        name: g.name_cstr.as_ptr(),
                                        file_count: g.files.len() as i32,
                                        paused_file_count: g.paused_file_count,
                                        size: g.size,
                                        remaining_size: g.remaining_size,
                                        paused_size: g.paused_size,
                                        stand_by: summary.stand_by,
                                        pause_download: summary.pause_download,
                                        current_download_speed: summary.current_download_speed,
                                    };
                                    let (row_buf, color) = format_group_row(
                                        &spec,
                                        &mut col_width_files,
                                        &mut col_width_total,
                                        &mut col_width_left,
                                    );
                                    term_plot_line(
                                        &row_buf,
                                        line_nr,
                                        0,
                                        screen_width,
                                        use_color,
                                        color,
                                    );
                                    line_nr += 1;
                                }
                                remaining += g.remaining_size;
                                paused += g.paused_size;
                            }

                            let hdr = format_group_queue_header(
                                use_color,
                                groups.len() as i32,
                                remaining,
                                paused,
                            );
                            let top =
                                format_top_header(&hdr, screen_width, false, summary.dn_time_sec);
                            term_plot_line(
                                &top,
                                queue_win_top,
                                0,
                                screen_width,
                                use_color,
                                COLOR_INFOLINE,
                            );
                        }
                    } else {
                        let mut line_nr = queue_win_top + 1;
                        let mut remaining: i64 = 0;
                        let mut paused: i64 = 0;
                        let mut paused_files: i32 = 0;
                        let mut file_num: i32 = 0;

                        for g in &groups {
                            for f in &g.files {
                                if in_scroll(file_num) {
                                    let spec = NcursesFileRowSpec {
                                        edit_queue_mode: i32::from(input_mode == 1),
                                        selected: i32::from(file_num == selected_queue_entry),
                                        use_color: i32::from(use_color),
                                        color_highl: COLOR_TEXTHIGHL,
                                        color_text: COLOR_TEXT,
                                        id: f.id,
                                        priority: g.priority,
                                        active_downloads: f.active_downloads,
                                        paused: i32::from(f.paused),
                                        show_nzbname: i32::from(show_nzbname),
                                        path_sep: env.path_sep,
                                        nzb_name: g.name_cstr.as_ptr(),
                                        filename: f.filename_cstr.as_ptr(),
                                        size: f.size,
                                        remaining_size: f.remaining_size,
                                    };
                                    let (row_buf, color) = format_file_row(&spec);
                                    term_plot_line(
                                        &row_buf,
                                        line_nr,
                                        0,
                                        screen_width,
                                        use_color,
                                        color,
                                    );
                                    line_nr += 1;
                                }
                                file_num += 1;
                                remaining += f.remaining_size;
                                if f.paused {
                                    paused += f.remaining_size;
                                    paused_files += 1;
                                }
                            }
                        }

                        let hdr = format_file_queue_header(
                            use_color,
                            file_num,
                            paused_files,
                            remaining,
                            paused,
                        );
                        let top = format_top_header(&hdr, screen_width, true, summary.up_time_sec);
                        term_plot_line(
                            &top,
                            queue_win_top,
                            0,
                            screen_width,
                            use_color,
                            COLOR_INFOLINE,
                        );
                        if file_num == 0 {
                            term_plot_line(
                                b"Ready to receive nzb-job",
                                line_nr,
                                0,
                                screen_width,
                                use_color,
                                COLOR_TEXT,
                            );
                        }
                    }
                }

                if messages_win_height > 0 {
                    let mut line_nr = messages_win_top;
                    term_plot_line(
                        if use_color {
                            b" Messages"
                        } else {
                            b"***  Messages"
                        },
                        line_nr,
                        0,
                        screen_width,
                        use_color,
                        COLOR_INFOLINE,
                    );
                    line_nr += 1;

                    let mut line = line_nr + messages_win_client_height - 1;
                    let mut lines_to_print = messages_win_client_height;

                    for m in messages.iter().rev() {
                        if lines_to_print <= 0 {
                            break;
                        }
                        let printed = ncurses_print_message(
                            m.kind,
                            m.time,
                            &m.text,
                            line,
                            lines_to_print,
                            show_timestamp,
                            env.time_correction,
                            screen_width,
                            use_color,
                        );
                        line -= printed;
                        lines_to_print -= printed;
                    }

                    if lines_to_print > 0 {
                        line = line_nr + messages_win_client_height - 1;
                        while lines_to_print > 0 {
                            lines_to_print -= 1;
                            term_plot_line(b"", line, 0, screen_width, use_color, COLOR_TEXT);
                            line -= 1;
                        }
                        let mut lines_to_print2 = messages_win_client_height;
                        for m in messages.iter().rev() {
                            if lines_to_print2 <= 0 {
                                break;
                            }
                            let printed = ncurses_print_message(
                                m.kind,
                                m.time,
                                &m.text,
                                line,
                                lines_to_print2,
                                show_timestamp,
                                env.time_correction,
                                screen_width,
                                use_color,
                            );
                            line -= printed;
                            lines_to_print2 -= printed;
                        }
                    }
                }

                let status_spec = NcursesStatusSpec {
                    stand_by: summary.stand_by,
                    pause_download: summary.pause_download,
                    current_download_speed: summary.current_download_speed,
                    remaining_size: summary.remaining_size,
                    download_limit: i64::from(summary.download_limit),
                    post_job_count: summary.post_job_count,
                    dn_time_sec: summary.dn_time_sec,
                    all_bytes: summary.all_bytes,
                    thread_count: summary.thread_count,
                };
                let status_buf = format_status_bar(&status_spec);
                term_plot_line(
                    &status_buf,
                    screen_height - 2,
                    0,
                    screen_width,
                    use_color,
                    COLOR_STATUS,
                );

                let cur_time = fe_action(env, 7, 0, 0, 0);
                ncurses_print_key_input_bar(
                    screen_height,
                    screen_width,
                    use_color,
                    input_mode,
                    group_files,
                    selected_queue_entry,
                    fe_calc_queue_size(&groups, group_files),
                    input_value,
                    input_number_index,
                    &hint,
                    &mut start_hint,
                    cur_time,
                );

                let mut k = key;
                while k != -1 {
                    let mut st = NcursesKeyState {
                        input_mode,
                        show_nzbname: i32::from(show_nzbname),
                        show_timestamp: i32::from(show_timestamp),
                        group_files: i32::from(group_files),
                        queue_window_percentage,
                        selected_queue_entry,
                        queue_scroll_offset,
                        queue_win_client_height,
                        input_number_index,
                        input_value,
                        ..Default::default()
                    };
                    ncurses_handle_key(&mut st, k, fe_calc_queue_size(&groups, group_files));
                    input_mode = st.input_mode;
                    show_nzbname = st.show_nzbname != 0;
                    show_timestamp = st.show_timestamp != 0;
                    group_files = st.group_files != 0;
                    queue_window_percentage = st.queue_window_percentage;
                    queue_scroll_offset = st.queue_scroll_offset;
                    input_number_index = st.input_number_index;
                    input_value = st.input_value;
                    if st.exit_proc != 0 {
                        let _ = fe_action(env, 6, 0, 0, 0);
                    }
                    if st.calc_window_sizes != 0 {
                        ncurses_calc_window_sizes(
                            fe_calc_queue_size(&groups, group_files),
                            queue_window_percentage,
                            &mut screen_height,
                            &mut screen_width,
                            &mut queue_win_top,
                            &mut queue_win_height,
                            &mut queue_win_client_height,
                            &mut messages_win_top,
                            &mut messages_win_height,
                            &mut messages_win_client_height,
                        );
                    }
                    if st.toggle_pause != 0 {
                        let pause = summary.pause_download == 0;
                        fe_server_pause_unpause(env, pause, env.remote_mode == 0);
                    }
                    if st.set_delete_hint != 0 {
                        hint.clear();
                        hint.extend_from_slice(b" Use Uppercase \"D\" for delete");
                        start_hint = fe_action(env, 7, 0, 0, 0);
                    }
                    let mut edit_ok = true;
                    if st.edit_action != 0 {
                        let mut act = st.edit_action;
                        let mut target_id = 0i32;
                        if group_files {
                            let has_group = selected_queue_entry >= 0
                                && (selected_queue_entry as usize) < groups.len();
                            let (rem, psd, rpc) = if has_group {
                                let g = &groups[selected_queue_entry as usize];
                                target_id = g.id;
                                (g.remaining_size, g.paused_size, g.remaining_par_count)
                            } else {
                                (0, 0, 0)
                            };
                            act = ncurses_resolve_group_edit_action(
                                act,
                                has_group,
                                rem,
                                psd,
                                rpc,
                                selected_queue_entry,
                                last_edit_entry,
                                &mut last_pause_pars,
                            );
                        } else {
                            let mut fnum = 0i32;
                            for g in &groups {
                                for f in &g.files {
                                    if selected_queue_entry == fnum {
                                        target_id = f.id;
                                        if act == 4 && f.paused {
                                            act = 5;
                                        }
                                    }
                                    fnum += 1;
                                }
                            }
                        }
                        last_edit_entry = selected_queue_entry;
                        data_update_pos = 10;
                        update_next_time = true;
                        edit_ok = if target_id != 0 {
                            let ok = fe_server_edit_queue(env, act, st.edit_offset, target_id);
                            if env.remote_mode == 0 {
                                groups = fe_collect_local_queue(env);
                            }
                            ok
                        } else {
                            false
                        };
                    }
                    if st.has_select_entry != 0 && edit_ok {
                        let (sel, scr) = ncurses_set_current_queue_entry(
                            st.select_entry,
                            fe_calc_queue_size(&groups, group_files),
                            queue_scroll_offset,
                            queue_win_client_height,
                        );
                        selected_queue_entry = sel;
                        queue_scroll_offset = scr;
                    }
                    if st.need_update_data != 0 {
                        data_update_pos = 10;
                        update_next_time = true;
                    }
                    if st.has_set_download_rate != 0 {
                        fe_server_set_rate(env, st.download_rate_bytes);
                    }
                    if st.stop_loop != 0 {
                        break;
                    }
                    k = read_key();
                }

                refresh_screen(screen_height, screen_width);
            }
        }

        if data_update_pos <= 0 {
            data_update_pos = env.update_interval;
        }
        let sleep_interval = if input_mode == 0 { 100 } else { 10 };
        let _ = fe_action(env, 1, sleep_interval, 0, 0);
        data_update_pos -= sleep_interval;
    }
}

/// Consolidated entry point for `nzbget_rs_frontend_op`:
/// - `0`: `term_init()`
/// - `1`: `term_shutdown()`
/// - `2`: `LoggableFrontend(false)`
/// - `3`: `LoggableFrontend(true)` (`ColoredFrontend`)
/// - `4`: `NCursesFrontend`
/// - `5`: `NServFrontend`
///
/// # Safety
/// For `op >= 2`, `env` must be a valid pointer to `NzbgetRsFrontendEnv`.
pub unsafe fn frontend_op(op: std::ffi::c_int, env: *const NzbgetRsFrontendEnv) -> std::ffi::c_int {
    match op {
        0 => term_init(),
        1 => {
            term_shutdown();
            0
        }
        2 => {
            if let Some(e) = env.as_ref() {
                run_loggable(0, e);
            }
            0
        }
        3 => {
            if let Some(e) = env.as_ref() {
                run_loggable(1, e);
            }
            0
        }
        4 => {
            if let Some(e) = env.as_ref() {
                run_ncurses(e, term_read_key, term_refresh);
            }
            0
        }
        5 => {
            if let Some(e) = env.as_ref() {
                run_loggable(3, e);
            }
            0
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontend_runner_loggable_colored_and_ncurses_local_loop() {
        struct MockHost {
            stopped: bool,
            waits: usize,
            max_waits: usize,
            flushes: usize,
            stdout: Vec<u8>,
            edited: Vec<(i32, i32, i32)>,
            paused_calls: Vec<(i32, i32)>,
            exit_called: bool,
        }
        unsafe extern "C" fn write_cb(
            raw: *mut std::ffi::c_void,
            ptr: *const std::ffi::c_char,
            len: usize,
            do_flush: std::ffi::c_int,
        ) {
            let h = &mut *raw.cast::<MockHost>();
            if !ptr.is_null() && len > 0 {
                h.stdout
                    .extend_from_slice(std::slice::from_raw_parts(ptr.cast::<u8>(), len));
            }
            if do_flush != 0 {
                h.flushes += 1;
            }
        }
        unsafe extern "C" fn action_cb(
            raw: *mut std::ffi::c_void,
            op: std::ffi::c_int,
            a1: std::ffi::c_int,
            a2: std::ffi::c_int,
            a3: std::ffi::c_int,
        ) -> i64 {
            let h = &mut *raw.cast::<MockHost>();
            match op {
                0 => i64::from(h.stopped),
                1 => {
                    h.waits += 1;
                    if h.waits >= h.max_waits {
                        h.stopped = true;
                    }
                    0
                }
                2 => {
                    h.stopped = true;
                    0
                }
                3 => {
                    h.paused_calls.push((a1, a2));
                    0
                }
                5 => {
                    h.edited.push((a1, a2, a3));
                    1
                }
                6 => {
                    h.exit_called = true;
                    h.stopped = true;
                    0
                }
                7 => 1_700_000_000,
                _ => 0,
            }
        }
        unsafe extern "C" fn sum_cb(
            _raw: *mut std::ffi::c_void,
            s: *mut crate::binrpc::FrontendSummary,
        ) {
            let s = &mut *s;
            s.current_download_speed = 1024 * 1024;
            s.remaining_size = 4 * 1024 * 1024;
            s.thread_count = 4;
            s.up_time_sec = 120;
            s.dn_time_sec = 60;
            s.all_bytes = 60 * 1024 * 1024;
        }
        unsafe extern "C" fn msgs_cb(
            _raw: *mut std::ffi::c_void,
            tail_mode: std::ffi::c_int,
            arg: u32,
            _out_skipped: *mut std::ffi::c_int,
            emit_ctx: *mut std::ffi::c_void,
            emit: FrontendEmitMsgFn,
        ) {
            if tail_mode != 0 || arg == 0 {
                emit(emit_ctx, 1, 0, 1_700_000_000, c"hello frontend".as_ptr());
            }
        }
        unsafe extern "C" fn queue_cb(
            _raw: *mut std::ffi::c_void,
            emit_ctx: *mut std::ffi::c_void,
            emit_nzb: FrontendEmitNzbFn,
            emit_file: FrontendEmitFileFn,
        ) {
            emit_nzb(
                emit_ctx,
                10,
                0,
                1,
                8 * 1024 * 1024,
                4 * 1024 * 1024,
                0,
                0,
                1,
                c"TestJob".as_ptr(),
            );
            emit_file(
                emit_ctx,
                0,
                101,
                1,
                0,
                8 * 1024 * 1024,
                4 * 1024 * 1024,
                c"part1.rar".as_ptr(),
            );
        }

        // 1. ColoredFrontend local loop
        let mut host = MockHost {
            stopped: false,
            waits: 0,
            max_waits: 2,
            flushes: 0,
            stdout: Vec::new(),
            edited: Vec::new(),
            paused_calls: Vec::new(),
            exit_called: false,
        };
        let env = NzbgetRsFrontendEnv {
            ctx: (&mut host as *mut MockHost).cast(),
            remote_mode: 0,
            update_interval: 50,
            use_color: 1,
            show_nzbname: 1,
            show_timestamp: 0,
            group_files: 0,
            time_correction: 0,
            path_sep: b'/',
            control_ip: c"127.0.0.1".as_ptr(),
            control_port: 6789,
            control_username: c"".as_ptr(),
            control_password: c"".as_ptr(),
            write_stdout_cb: Some(write_cb),
            action_cb: Some(action_cb),
            local_summary_cb: Some(sum_cb),
            local_messages_cb: Some(msgs_cb),
            local_queue_cb: Some(queue_cb),
        };
        unsafe { frontend_op(3, &env) };
        let text = String::from_utf8(host.stdout.clone()).unwrap();
        assert!(text.contains("\x1b[32m[INFO]\x1b[39m hello frontend\x1b[K\n"));
        assert!(text.contains("4 threads, 1.00 MB/s, 4.00 MB remaining"));
        assert!(text.contains("\r\x1b[1A"));
        assert!(host.flushes >= 2);

        // 2. NCursesFrontend local loop with synthetic keys ('p', 'e', 'p', 'q')
        host.stopped = false;
        host.waits = 0;
        host.max_waits = 10;
        let mut keys = std::collections::VecDeque::from([
            b'p' as i32,
            b'e' as i32,
            -1,
            b'p' as i32,
            b'q' as i32,
            -1,
        ]);
        let mut rendered = Vec::new();
        run_ncurses(
            &env,
            || keys.pop_front().unwrap_or(-1),
            |_r, _c| {
                let mut g = TERM_STATE.lock().unwrap_or_else(|e| e.into_inner());
                rendered.extend_from_slice(&g.frame_buf);
                g.frame_buf.clear();
            },
        );
        let frame_str = String::from_utf8(rendered).unwrap();
        assert!(frame_str.contains("TestJob/part1.rar"));
        assert!(frame_str.contains("hello frontend"));
        assert_eq!(host.paused_calls, vec![(1, 1)]);
        assert_eq!(host.edited, vec![(4, 0, 101)]);
        assert!(host.exit_called);
    }

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
