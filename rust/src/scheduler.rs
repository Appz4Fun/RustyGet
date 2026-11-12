//! Scheduler::CheckTasks' timing: which scheduled tasks are due between the
//! last check and now (local time), day by day over the check interval, with the
//! startup tasks and the reset after a clock jump. Running the tasks stays
//! C++. Dates are computed as the C++ did: gmtime_r's fields and nzbget's
//! Timegm (Boost's arithmetic, without normalizing the fields).

/// A task's schedule and its last run (0: never); NzbgetRsSchedTask in C.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Task {
    /// -1 for a startup task
    pub hours: i32,
    pub minutes: i32,
    /// bit n: weekday n + 1 (1 Monday .. 7 Sunday); 0: every day
    pub week_days: i32,
    pub last_executed: i64,
}

pub const STARTUP_TASK: i32 = -1;

/// The fields of gmtime_r used here.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tm {
    pub year: i64,
    /// 0..=11
    pub mon: i64,
    pub mday: i64,
    pub hour: i64,
    pub min: i64,
    pub sec: i64,
    /// 0 Sunday ..= 6
    pub wday: i64,
}

/// gmtime_r: a time as UTC calendar fields.
/// Only for tests using POSIX time; production uses the caller's libc, which
/// may apply leap seconds from the process timezone's TZif file.
#[cfg(test)]
pub fn gmtime(t: i64) -> Tm {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    // civil_from_days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let mday = doy - (153 * mp + 2) / 5 + 1;
    let mon = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if mon <= 2 { 1 } else { 0 };
    Tm {
        year,
        mon: mon - 1,
        mday,
        hour: secs / 3600,
        min: secs % 3600 / 60,
        sec: secs % 60,
        wday: (days + 4).rem_euclid(7),
    }
}

fn is_leap(year: i64) -> bool {
    year % 400 == 0 || (year % 100 != 0 && year % 4 == 0)
}

fn days_from_0(year: i64) -> i64 {
    let y = year - 1;
    // C integer division (truncating), as the C++ int arithmetic
    365 * y + y / 400 - y / 100 + y / 4
}

/// nzbget's Timegm (internal_timegm, from Boost).
pub fn timegm(t: &Tm) -> i64 {
    const DAYS: [[i64; 12]; 2] = [
        [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334],
        [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335],
    ];
    let mut year = t.year;
    let mut month = t.mon;
    if month > 11 {
        year += month / 12;
        month %= 12;
    } else if month < 0 {
        let diff = (-month + 11) / 12;
        year -= diff;
        month += 12 * diff;
    }
    let day_of_year = DAYS[is_leap(year) as usize][month as usize] + t.mday - 1;
    let days = days_from_0(year) - 719162 + day_of_year;
    86400 * days + 3600 * t.hour + 60 * t.min + t.sec
}

/// The outcome of a check.
#[derive(Debug, PartialEq)]
pub struct Check {
    /// the tasks to run, by index, in order
    pub due: Vec<usize>,
    /// the clock jumped (more than 90 minutes or back): processes the tasks
    /// start aren't run for the past (m_executeProcess = false)
    pub reset: bool,
}

/// Scheduler::CheckTasks' timing for `tasks` (in the scheduler's order):
/// updates their last runs and `last_check` and returns what to run.
/// Nine entries per task normally suffice, but libc's TZif leap corrections
/// can repeat calendar dates and make this bound invalid.
pub fn check_tasks(
    tasks: &mut [Task],
    last_check: &mut i64,
    current: i64,
    current_offset: i64,
    last_check_offset: i64,
    gmtime: impl Fn(i64) -> Tm,
) -> Check {
    let mut r = Check {
        due: Vec::new(),
        reset: false,
    };
    if !tasks.is_empty() {
        let diff = current - *last_check;
        if !(0..=60 * 90).contains(&diff) {
            // check all tasks for the last week
            *last_check = current - 60 * 60 * 24 * 7;
            r.reset = true;
            for t in tasks.iter_mut().filter(|t| t.hours != STARTUP_TASK) {
                t.last_executed = 0;
            }
        }
        let local_current = current + current_offset;
        let local_last_check = *last_check + last_check_offset;
        let tm_current = gmtime(local_current);
        let mut tm_loop = gmtime(local_last_check);
        tm_loop.hour = tm_current.hour;
        tm_loop.min = tm_current.min;
        tm_loop.sec = tm_current.sec;
        let mut lp = timegm(&tm_loop);
        while lp <= local_current {
            for (k, task) in tasks.iter_mut().enumerate() {
                if task.last_executed == lp {
                    continue;
                }
                // the loop day's date with the task's time (the weekday is the
                // loop day's: Timegm doesn't recompute it)
                let appoint = timegm(&Tm {
                    hour: task.hours as i64,
                    min: task.minutes as i64,
                    sec: 0,
                    ..tm_loop
                });
                let week_day = if tm_loop.wday == 0 { 7 } else { tm_loop.wday };
                let week_ok = task.week_days == 0 || task.week_days & (1 << (week_day - 1)) != 0;
                let due = (task.hours >= 0
                    && week_ok
                    && local_last_check < appoint
                    && appoint <= local_current)
                    || (task.hours == STARTUP_TASK && task.last_executed == 0);
                if due {
                    r.due.push(k);
                    task.last_executed = lp;
                }
            }
            lp += 60 * 60 * 24;
            tm_loop = gmtime(lp);
        }
    }
    *last_check = current;
    r
}

/// `Scheduler::FirstCheck`:
/// Comparator ordering scheduler tasks by `(hours, minutes)`.
#[inline]
pub fn scheduler_task_less(hours1: i32, minutes1: i32, hours2: i32, minutes2: i32) -> bool {
    (hours1 < hours2) || ((hours1 == hours2) && (minutes1 < minutes2))
}

/// `Scheduler::ExecuteTask`:
/// Checks whether process/extension/feed commands should be executed.
#[inline]
pub fn scheduler_should_execute_process(
    execute_process: bool,
    task_hours: i32,
    startup_task: i32,
) -> bool {
    execute_process || task_hours == startup_task
}

/// `Scheduler::ExecuteTask`:
/// Converts KB/s speed limit to bytes/s.
#[inline]
pub fn scheduler_calc_speed_limit_bytes(kb_per_sec: i32) -> i32 {
    kb_per_sec.wrapping_mul(1024)
}

/// `Scheduler::EditServer`:
/// Checks whether a server matches `server_ref` by numeric ID or case-insensitive name.
///
/// # Safety
/// `server_name` and `server_ref` must be valid null-terminated C strings (or null).
#[inline]
pub unsafe fn scheduler_matches_server_ref(
    id: i32,
    server_id: i32,
    server_name: *const std::ffi::c_char,
    server_ref: *const std::ffi::c_char,
) -> bool {
    if id > 0 && server_id == id {
        return true;
    }
    if server_name.is_null() || server_ref.is_null() {
        return false;
    }
    std::ffi::CStr::from_ptr(server_name)
        .to_bytes()
        .eq_ignore_ascii_case(std::ffi::CStr::from_ptr(server_ref).to_bytes())
}

/// `Scheduler::FetchFeed`:
/// Checks whether a feed matches `feed_ref` by numeric ID, case-insensitive name, or `"0"`,
/// and resolves the target feed ID to fetch (`0` for `"0"`, else `feed_id`).
///
/// # Safety
/// `feed_name` and `feed_ref` must be valid null-terminated C strings (or null).
#[inline]
pub unsafe fn scheduler_matches_feed_ref(
    id: i32,
    feed_id: i32,
    feed_name: *const std::ffi::c_char,
    feed_ref: *const std::ffi::c_char,
) -> (bool, i32) {
    if feed_ref.is_null() {
        return (feed_id == id, feed_id);
    }
    let ref_bytes = std::ffi::CStr::from_ptr(feed_ref).to_bytes();
    let is_zero = ref_bytes == b"0";
    let matched = feed_id == id
        || (!feed_name.is_null()
            && std::ffi::CStr::from_ptr(feed_name)
                .to_bytes()
                .eq_ignore_ascii_case(ref_bytes))
        || is_zero;
    let target_id = if is_zero { 0 } else { feed_id };
    (matched, target_id)
}

/// `Scheduler::CheckScheduledResume`:
/// Checks whether scheduled auto-resume is due.
#[inline]
pub fn scheduler_should_autoresume(resume_time: i64, current_time: i64) -> bool {
    resume_time > 0 && current_time >= resume_time
}

/// Converts bytes per second to KB/s (`bytes / 1024`) for `Scheduler::PrintLog`.
pub fn scheduler_bytes_to_kb(bytes: i32) -> i32 {
    bytes / 1024
}

/// Returns `"pausing"` or `"unpausing"` for `Scheduler::PrintLog`.
pub fn scheduler_pause_action_str(paused: bool) -> &'static [u8] {
    if paused {
        b"pausing\0"
    } else {
        b"unpausing\0"
    }
}

/// Returns `"activating"` or `"deactivating"` for `Scheduler::PrintLog`.
pub fn scheduler_server_action_str(active: bool) -> &'static [u8] {
    if active {
        b"activating\0"
    } else {
        b"deactivating\0"
    }
}

/// Returns the NUL-terminated `Scheduler::ExecuteTask` debug command name.
pub fn scheduler_command_name(command: i32) -> &'static [u8] {
    match command {
        0 => b"Pause\0",
        1 => b"Unpause\0",
        2 => b"Pause Post-processing\0",
        3 => b"Unpause Post-processing\0",
        4 => b"Set download rate\0",
        5 => b"Execute process\0",
        6 => b"Execute script\0",
        7 => b"Pause Scan\0",
        8 => b"Unpause Scan\0",
        9 => b"Enable Server\0",
        10 => b"Disable Server\0",
        11 => b"Fetch Feed\0",
        _ => b"Unknown\0",
    }
}

/// Computes seconds until the next UTC minute boundary (`gmtime_r` + `tm_min + 1`, `tm_sec = 0` + `timegm`)
/// for `Scheduler::ScheduleNextWork`.
pub fn scheduler_calc_next_work_interval(cur_time: i64) -> i32 {
    #[cfg(unix)]
    unsafe {
        let t = cur_time as libc::time_t;
        let mut fields: libc::tm = std::mem::zeroed();
        libc::gmtime_r(&t, &mut fields);
        let next = timegm(&Tm {
            year: fields.tm_year as i64 + 1900,
            mon: fields.tm_mon as i64,
            mday: fields.tm_mday as i64,
            hour: fields.tm_hour as i64,
            min: fields.tm_min as i64 + 1,
            sec: 0,
            wday: fields.tm_wday as i64,
        });
        (next - cur_time) as i32
    }
    #[cfg(not(unix))]
    {
        (60 - cur_time.rem_euclid(60)) as i32
    }
}

// ============================================================================
// End-to-end `Scheduler` coordinator (`daemon/main/Scheduler.cpp`)
// ============================================================================

use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

#[repr(C)]
pub struct NzbgetRsSchedulerEnv {
    pub ctx: *mut c_void,
    pub a1: c_int,
    pub a2: c_int,
    pub a3: c_int,
    pub a4: c_int,
    pub a5: c_int,
    pub s1: *const c_char,
    pub log_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    pub action_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            c_int,
            c_int,
            *const *const c_char,
            usize,
            *mut c_int,
            *mut i64,
            *mut *const c_char,
        ) -> c_int,
    >,
}

struct SchedTaskEntry {
    id: i32,
    hours: i32,
    minutes: i32,
    week_days_bits: i32,
    command: i32,
    param: Vec<u8>,
    last_executed: i64,
}

#[derive(Default)]
struct SchedulerWorkState {
    last_check: i64,
    first_checked: bool,
    execute_process: bool,
    download_rate_changed: bool,
    pause_download_changed: bool,
    pause_post_process_changed: bool,
    pause_scan_changed: bool,
    server_changed: bool,
    server_status_list: Vec<bool>,
}

pub struct NzbgetRsScheduler {
    tasks: Mutex<Vec<SchedTaskEntry>>,
    work: Mutex<SchedulerWorkState>,
    service_interval: AtomicI32,
}

fn make_c_buf(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut v = Vec::with_capacity(end + 1);
    v.extend_from_slice(&bytes[..end]);
    v.push(0);
    v
}

unsafe fn emit_sched_log(env: &NzbgetRsSchedulerEnv, level: c_int, msg: &[u8]) {
    if let Some(cb) = env.log_cb {
        let cbuf = make_c_buf(msg);
        cb(env.ctx, level, cbuf.as_ptr() as *const c_char);
    }
}

fn libc_gmtime_tm(val: i64) -> Tm {
    #[cfg(unix)]
    unsafe {
        let t = val as libc::time_t;
        let mut fields: libc::tm = std::mem::zeroed();
        libc::gmtime_r(&t, &mut fields);
        Tm {
            year: fields.tm_year as i64 + 1900,
            mon: fields.tm_mon as i64,
            mday: fields.tm_mday as i64,
            hour: fields.tm_hour as i64,
            min: fields.tm_min as i64,
            sec: fields.tm_sec as i64,
            wday: fields.tm_wday as i64,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = val;
        Tm {
            year: 1970,
            mon: 0,
            mday: 1,
            hour: 0,
            min: 0,
            sec: 0,
            wday: 4,
        }
    }
}

fn current_unix_time(env: &NzbgetRsSchedulerEnv) -> i64 {
    if let Some(cb) = env.action_cb {
        let mut i64s = [0i64; 2];
        unsafe {
            cb(
                env.ctx,
                6,
                0,
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                i64s.as_mut_ptr(),
                std::ptr::null_mut(),
            );
        }
        if i64s[0] != 0 {
            return i64s[0];
        }
    }
    #[cfg(unix)]
    unsafe {
        libc::time(std::ptr::null_mut()) as i64
    }
    #[cfg(not(unix))]
    {
        0
    }
}

impl NzbgetRsScheduler {
    fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            work: Mutex::new(SchedulerWorkState::default()),
            service_interval: AtomicI32::new(1),
        }
    }

    fn add_task(
        &self,
        id: i32,
        hours: i32,
        minutes: i32,
        week_days_bits: i32,
        command: i32,
        param: &[u8],
    ) {
        let mut guard = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        guard.push(SchedTaskEntry {
            id,
            hours,
            minutes,
            week_days_bits,
            command,
            param: param.to_vec(),
            last_executed: 0,
        });
    }

    pub(crate) fn collect_health_tasks(&self) -> Vec<crate::systemhealth::HealthTaskInput> {
        let guard = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .iter()
            .map(|t| crate::systemhealth::HealthTaskInput {
                id: t.id,
                hours: t.hours,
                minutes: t.minutes,
                week_days_bits: t.week_days_bits,
            })
            .collect()
    }

    unsafe fn execute_task(
        ws: &mut SchedulerWorkState,
        task: &SchedTaskEntry,
        is_debug: bool,
        env: &NzbgetRsSchedulerEnv,
    ) {
        let Some(cb) = env.action_cb else {
            return;
        };
        if is_debug {
            let cmd_bytes = scheduler_command_name(task.command);
            let cmd_slice = &cmd_bytes[..cmd_bytes.len().saturating_sub(1)];
            let mut msg = b"Executing scheduled command: ".to_vec();
            msg.extend_from_slice(cmd_slice);
            emit_sched_log(env, 0, &msg);
        }

        let exec_proc =
            scheduler_should_execute_process(ws.execute_process, task.hours, STARTUP_TASK);
        let param_c = make_c_buf(&task.param);

        match task.command {
            4 => {
                // scDownloadRate
                if !task.param.is_empty() {
                    let kb = libc::atoi(param_c.as_ptr() as *const c_char);
                    let limit = scheduler_calc_speed_limit_bytes(kb);
                    cb(
                        env.ctx,
                        1,
                        0,
                        limit,
                        std::ptr::null(),
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                    ws.download_rate_changed = true;
                }
            }
            0 | 1 => {
                // scPauseDownload | scUnpauseDownload
                cb(
                    env.ctx,
                    1,
                    1,
                    c_int::from(task.command == 0),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                ws.pause_download_changed = true;
            }
            2 | 3 => {
                // scPausePostProcess | scUnpausePostProcess
                cb(
                    env.ctx,
                    1,
                    2,
                    c_int::from(task.command == 2),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                ws.pause_post_process_changed = true;
            }
            7 | 8 => {
                // scPauseScan | scUnpauseScan
                cb(
                    env.ctx,
                    1,
                    3,
                    c_int::from(task.command == 7),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                ws.pause_scan_changed = true;
            }
            5 | 6 => {
                // scExtensions | scProcess
                if exec_proc {
                    if task.command == 6 {
                        let words = crate::util::split_command_line(&task.param);
                        if words.is_empty() {
                            let mut err = b"Could not execute scheduled process-script, failed to parse command line: ".to_vec();
                            err.extend_from_slice(&task.param);
                            emit_sched_log(env, 4, &err);
                        } else {
                            let c_words: Vec<Vec<u8>> =
                                words.iter().map(|w| make_c_buf(w)).collect();
                            let mut ptrs: Vec<*const c_char> =
                                Vec::with_capacity(c_words.len() + 1);
                            ptrs.push(param_c.as_ptr() as *const c_char);
                            for w in &c_words {
                                ptrs.push(w.as_ptr() as *const c_char);
                            }
                            cb(
                                env.ctx,
                                4,
                                1,
                                task.id,
                                ptrs.as_ptr(),
                                ptrs.len(),
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                            );
                        }
                    } else {
                        let ptrs = [param_c.as_ptr() as *const c_char];
                        cb(
                            env.ctx,
                            4,
                            0,
                            task.id,
                            ptrs.as_ptr(),
                            1,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        );
                    }
                }
            }
            9 | 10 => {
                // scActivateServer | scDeactivateServer
                Self::edit_server(ws, task.command == 9, &task.param, env);
            }
            11 => {
                // scFetchFeed
                if exec_proc {
                    let ptrs = [param_c.as_ptr() as *const c_char];
                    cb(
                        env.ctx,
                        3,
                        0,
                        0,
                        ptrs.as_ptr(),
                        1,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                }
            }
            _ => {}
        }
    }

    unsafe fn edit_server(
        ws: &mut SchedulerWorkState,
        active: bool,
        server_list: &[u8],
        env: &NzbgetRsSchedulerEnv,
    ) {
        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            let srvs = pool.servers_snapshot();
            let mut pos = 0usize;
            while pos <= server_list.len() {
                let Some((tok_start, tok_end, next_pos)) =
                    crate::util::tokenizer_find_next_token(&server_list[pos..], b",;")
                else {
                    break;
                };
                let tok = &server_list[pos + tok_start..pos + tok_end];
                pos += next_pos;
                let ref_c = make_c_buf(tok);
                let ref_id = libc::atoi(ref_c.as_ptr() as *const c_char);

                for s in &srvs {
                    if scheduler_matches_server_ref(
                        ref_id,
                        s.id,
                        s.name.as_ptr(),
                        ref_c.as_ptr() as *const c_char,
                    ) {
                        if !ws.server_changed {
                            ws.server_status_list = srvs
                                .iter()
                                .map(|srv| srv.active.load(Ordering::SeqCst))
                                .collect();
                        }
                        ws.server_changed = true;
                        s.active.store(active, Ordering::SeqCst);
                        break;
                    }
                }
            }
            return;
        }

        let Some(cb) = env.action_cb else {
            return;
        };
        let mut pos = 0usize;
        while pos <= server_list.len() {
            let Some((tok_start, tok_end, next_pos)) =
                crate::util::tokenizer_find_next_token(&server_list[pos..], b",;")
            else {
                break;
            };
            let tok = &server_list[pos + tok_start..pos + tok_end];
            pos += next_pos;
            let ref_c = make_c_buf(tok);
            let ref_id = libc::atoi(ref_c.as_ptr() as *const c_char);

            let srv_count = cb(
                env.ctx,
                2,
                0,
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            for idx in 0..srv_count {
                let mut ints = [0i32; 2];
                let mut strs: [*const c_char; 1] = [std::ptr::null()];
                cb(
                    env.ctx,
                    2,
                    1,
                    idx,
                    std::ptr::null(),
                    0,
                    ints.as_mut_ptr(),
                    std::ptr::null_mut(),
                    strs.as_mut_ptr(),
                );
                if scheduler_matches_server_ref(
                    ref_id,
                    ints[0],
                    strs[0],
                    ref_c.as_ptr() as *const c_char,
                ) {
                    if !ws.server_changed {
                        ws.server_status_list.clear();
                        ws.server_status_list.reserve(srv_count as usize);
                        for idx2 in 0..srv_count {
                            let mut ints2 = [0i32; 2];
                            let mut strs2: [*const c_char; 1] = [std::ptr::null()];
                            cb(
                                env.ctx,
                                2,
                                1,
                                idx2,
                                std::ptr::null(),
                                0,
                                ints2.as_mut_ptr(),
                                std::ptr::null_mut(),
                                strs2.as_mut_ptr(),
                            );
                            ws.server_status_list.push(ints2[1] != 0);
                        }
                    }
                    ws.server_changed = true;
                    let mut flag = [c_int::from(active)];
                    cb(
                        env.ctx,
                        2,
                        2,
                        idx,
                        std::ptr::null(),
                        0,
                        flag.as_mut_ptr(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                    break;
                }
            }
        }
    }

    unsafe fn check_tasks_and_log(
        &self,
        ws: &mut SchedulerWorkState,
        is_debug: bool,
        env: &NzbgetRsSchedulerEnv,
    ) {
        let Some(cb) = env.action_cb else {
            return;
        };
        ws.download_rate_changed = false;
        ws.pause_download_changed = false;
        ws.pause_post_process_changed = false;
        ws.pause_scan_changed = false;
        ws.server_changed = false;

        {
            let mut task_guard = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
            let current = current_unix_time(env);
            let mut sched_tasks: Vec<Task> = task_guard
                .iter()
                .map(|t| Task {
                    hours: t.hours,
                    minutes: t.minutes,
                    week_days: t.week_days_bits,
                    last_executed: t.last_executed,
                })
                .collect();

            let (current_offset, last_check_offset) = if sched_tasks.is_empty() {
                (0i64, 0i64)
            } else {
                let mut ints = [0i32; 6];
                let mut i64s = [0i64; 1];
                cb(
                    env.ctx,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    0,
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    std::ptr::null_mut(),
                );
                (ints[0] as i64, ints[1] as i64)
            };

            let mut last_check = ws.last_check;
            let res = check_tasks(
                &mut sched_tasks,
                &mut last_check,
                current,
                current_offset,
                last_check_offset,
                libc_gmtime_tm,
            );
            if res.reset {
                emit_sched_log(
                    env,
                    0,
                    b"Reset scheduled tasks (detected clock change greater than 90 minutes or negative)",
                );
                ws.execute_process = false;
            }
            for (i, st) in sched_tasks.iter().enumerate() {
                task_guard[i].last_executed = st.last_executed;
            }
            for &idx in &res.due {
                Self::execute_task(ws, &task_guard[idx], is_debug, env);
            }
            ws.last_check = last_check;
        }

        // PrintLog
        if ws.download_rate_changed
            || ws.pause_download_changed
            || ws.pause_post_process_changed
            || ws.pause_scan_changed
        {
            let mut ints = [0i32; 6];
            let mut i64s = [0i64; 1];
            cb(
                env.ctx,
                0,
                0,
                0,
                std::ptr::null(),
                0,
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            if ws.download_rate_changed {
                let msg = format!(
                    "Scheduler: setting download rate to {} KB/s",
                    scheduler_bytes_to_kb(ints[2])
                );
                emit_sched_log(env, 2, msg.as_bytes());
            }
            if ws.pause_download_changed {
                let act = scheduler_pause_action_str(ints[3] != 0);
                let mut msg = b"Scheduler: ".to_vec();
                msg.extend_from_slice(&act[..act.len() - 1]);
                msg.extend_from_slice(b" download");
                emit_sched_log(env, 2, &msg);
            }
            if ws.pause_post_process_changed {
                let act = scheduler_pause_action_str(ints[4] != 0);
                let mut msg = b"Scheduler: ".to_vec();
                msg.extend_from_slice(&act[..act.len() - 1]);
                msg.extend_from_slice(b" post-processing");
                emit_sched_log(env, 2, &msg);
            }
            if ws.pause_scan_changed {
                let act = scheduler_pause_action_str(ints[5] != 0);
                let mut msg = b"Scheduler: ".to_vec();
                msg.extend_from_slice(&act[..act.len() - 1]);
                msg.extend_from_slice(b" scan");
                emit_sched_log(env, 2, &msg);
            }
        }
        if ws.server_changed {
            #[cfg(unix)]
            let handled = if let Some(pool) = crate::net::active_server_pool() {
                let srvs = pool.servers_snapshot();
                for (idx, s) in srvs.iter().enumerate() {
                    let cur_active = s.active.load(Ordering::SeqCst);
                    if let Some(&prev_active) = ws.server_status_list.get(idx) {
                        if cur_active != prev_active {
                            let act = scheduler_server_action_str(cur_active);
                            let mut msg = b"Scheduler: ".to_vec();
                            msg.extend_from_slice(&act[..act.len() - 1]);
                            msg.push(b' ');
                            msg.extend_from_slice(s.name.to_bytes());
                            emit_sched_log(env, 2, &msg);
                        }
                    }
                }
                pool.changed();
                true
            } else {
                false
            };
            #[cfg(not(unix))]
            let handled = false;
            if !handled {
                let srv_count = cb(
                    env.ctx,
                    2,
                    0,
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                for idx in 0..srv_count {
                    let mut ints = [0i32; 2];
                    let mut strs: [*const c_char; 1] = [std::ptr::null()];
                    cb(
                        env.ctx,
                        2,
                        1,
                        idx,
                        std::ptr::null(),
                        0,
                        ints.as_mut_ptr(),
                        std::ptr::null_mut(),
                        strs.as_mut_ptr(),
                    );
                    let cur_active = ints[1] != 0;
                    if let Some(&prev_active) = ws.server_status_list.get(idx as usize) {
                        if cur_active != prev_active {
                            let act = scheduler_server_action_str(cur_active);
                            let name_bytes = if strs[0].is_null() {
                                &[][..]
                            } else {
                                CStr::from_ptr(strs[0]).to_bytes()
                            };
                            let mut msg = b"Scheduler: ".to_vec();
                            msg.extend_from_slice(&act[..act.len() - 1]);
                            msg.push(b' ');
                            msg.extend_from_slice(name_bytes);
                            emit_sched_log(env, 2, &msg);
                        }
                    }
                }
                cb(
                    env.ctx,
                    2,
                    3,
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
    }

    unsafe fn service_work(&self, env: &NzbgetRsSchedulerEnv) {
        let Some(cb) = env.action_cb else {
            return;
        };
        let is_debug = env.a5 != 0;
        emit_sched_log(env, 0, b"Scheduler service work");

        let mut ints = [0i32; 6];
        let mut i64s = [0i64; 1];
        if cb(
            env.ctx,
            0,
            0,
            0,
            std::ptr::null(),
            0,
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        ) == 0
        {
            return;
        }

        emit_sched_log(env, 0, b"Scheduler service work: doing work");

        let mut ws = self.work.lock().unwrap_or_else(|e| e.into_inner());
        if !ws.first_checked {
            {
                let mut task_guard = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
                task_guard.sort_by(|a, b| {
                    if scheduler_task_less(a.hours, a.minutes, b.hours, b.minutes) {
                        std::cmp::Ordering::Less
                    } else if scheduler_task_less(b.hours, b.minutes, a.hours, a.minutes) {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Equal
                    }
                });
            }
            self.check_tasks_and_log(&mut ws, is_debug, env);
            ws.first_checked = true;
            return;
        }

        ws.execute_process = true;
        self.check_tasks_and_log(&mut ws, is_debug, env);

        // CheckScheduledResume
        let mut resume_ints = [0i32; 6];
        let mut resume_i64s = [0i64; 1];
        cb(
            env.ctx,
            0,
            0,
            0,
            std::ptr::null(),
            0,
            resume_ints.as_mut_ptr(),
            resume_i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        let now_resume = current_unix_time(env);
        if scheduler_should_autoresume(resume_i64s[0], now_resume) {
            emit_sched_log(env, 2, b"Autoresume");
            cb(
                env.ctx,
                1,
                4,
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }

        // ScheduleNextWork
        let now_next = current_unix_time(env);
        self.service_interval.store(
            scheduler_calc_next_work_interval(now_next),
            Ordering::SeqCst,
        );
    }
}

/// Formats `(exec_message, info_name, log_prefix)` for `SchedulerScriptController`:
/// - `is_external = true`: scheduled `scProcess` external command (`script_or_name` = `GetScript()`).
/// - `is_external = false`: scheduled `scExtensions` script (`script_or_name` = `script->GetName()`, `display_name` = `script->GetDisplayName()`).
pub fn format_scheduler_script_labels(
    is_external: bool,
    task_id: i32,
    script_or_name: &[u8],
    display_name: &[u8],
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    if is_external {
        let base = &script_or_name[crate::paths::base_file_name(script_or_name)..];
        let suffix = format!(" for Task{}", task_id);

        let mut exec_msg = b"Executing scheduled process-script ".to_vec();
        exec_msg.extend_from_slice(base);
        exec_msg.extend_from_slice(suffix.as_bytes());

        let mut info_name = b"scheduled process-script ".to_vec();
        info_name.extend_from_slice(base);
        info_name.extend_from_slice(suffix.as_bytes());
        if info_name.len() > 1023 {
            info_name.truncate(1023);
        }

        let mut prefix = base.to_vec();
        if prefix.len() > 1023 {
            prefix.truncate(1023);
        }
        if let Some(dot) = prefix.iter().rposition(|&b| b == b'.') {
            prefix.truncate(dot);
        }
        (exec_msg, info_name, prefix)
    } else {
        let mut suffix = Vec::new();
        if task_id > 0 {
            let s = format!(" for Task{}", task_id);
            suffix.extend_from_slice(s.as_bytes());
            if suffix.len() > 1023 {
                suffix.truncate(1023);
            }
        }
        let mut exec_msg = b"Executing scheduler-script ".to_vec();
        exec_msg.extend_from_slice(script_or_name);
        exec_msg.extend_from_slice(&suffix);

        let mut info_name = b"scheduler-script ".to_vec();
        info_name.extend_from_slice(script_or_name);
        info_name.extend_from_slice(&suffix);
        if info_name.len() > 1023 {
            info_name.truncate(1023);
        }

        (exec_msg, info_name, display_name.to_vec())
    }
}

static ACTIVE_SCHEDULER: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsScheduler>>> =
    std::sync::RwLock::new(None);

pub(crate) fn active_scheduler() -> Option<std::sync::Arc<NzbgetRsScheduler>> {
    ACTIVE_SCHEDULER
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_scheduler_new() -> *mut NzbgetRsScheduler {
    let arc = std::sync::Arc::new(NzbgetRsScheduler::new());
    if let Ok(mut g) = ACTIVE_SCHEDULER.write() {
        *g = Some(std::sync::Arc::downgrade(&arc));
    }
    std::sync::Arc::into_raw(arc).cast_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_scheduler_free(h: *mut NzbgetRsScheduler) {
    if !h.is_null() {
        let arc = std::sync::Arc::from_raw(h.cast_const());
        let wk = std::sync::Arc::downgrade(&arc);
        if let Ok(mut g) = ACTIVE_SCHEDULER.write() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
            {
                *g = None;
            }
        }
        drop(arc);
    }
}

/// Executes a `Scheduler` operation:
/// - `op = 0` (`AddTask`): adds `(a1=id, a2=hours, a3=minutes, a4=weekDaysBits, a5=command, s1=param)`.
/// - `op = 1` (`ServiceInterval`): returns `m_serviceInterval`.
/// - `op = 2` (`ServiceWork`): executes `Scheduler::ServiceWork` (`env.a5 != 0` = debug build).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_scheduler_exec(
    h: *mut NzbgetRsScheduler,
    op: c_int,
    env: *const NzbgetRsSchedulerEnv,
) -> c_int {
    let Some(sched) = h.as_ref() else {
        return 0;
    };
    match op {
        0 => {
            let Some(env_ref) = env.as_ref() else {
                return 0;
            };
            let param = if env_ref.s1.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(env_ref.s1).to_bytes()
            };
            sched.add_task(
                env_ref.a1, env_ref.a2, env_ref.a3, env_ref.a4, env_ref.a5, param,
            );
            1
        }
        1 => sched.service_interval.load(Ordering::SeqCst),
        2 => {
            if let Some(env_ref) = env.as_ref() {
                sched.service_work(env_ref);
            }
            1
        }
        _ => 0,
    }
}

struct ServiceSlot {
    ptr: usize,
    last_work: i64,
}

pub struct NzbgetRsServiceCoord {
    services: Mutex<Vec<ServiceSlot>>,
    wait_mutex: Mutex<bool>,
    wait_cond: std::sync::Condvar,
    disk_service_attached: AtomicBool,
}

static ACTIVE_SERVICE_COORD: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsServiceCoord>>> =
    std::sync::RwLock::new(None);

pub(crate) fn wake_active_disk_service() {
    let Some(sc) = ACTIVE_SERVICE_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
    else {
        return;
    };
    if sc.disk_service_attached.load(Ordering::Acquire) {
        crate::log::log_active_bytes(3, b"Waking up ServiceCoordinator");
        if let Ok(mut woken) = sc.wait_mutex.lock() {
            *woken = true;
            sc.wait_cond.notify_all();
        }
    }
}

pub type NzbgetRsServiceCoordFn =
    unsafe extern "C" fn(ctx: *mut c_void, svc: *mut c_void, op: c_int) -> c_int;

#[inline]
fn service_coord_now() -> i64 {
    #[cfg(unix)]
    unsafe {
        libc::time(std::ptr::null_mut()) as i64
    }
    #[cfg(not(unix))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_service_coord_new() -> *mut NzbgetRsServiceCoord {
    crate::log::log_active_bytes(3, b"Creating ServiceCoordinator");
    let arc = std::sync::Arc::new(NzbgetRsServiceCoord {
        services: Mutex::new(Vec::new()),
        wait_mutex: Mutex::new(false),
        wait_cond: std::sync::Condvar::new(),
        disk_service_attached: AtomicBool::new(false),
    });
    if let Ok(mut g) = ACTIVE_SERVICE_COORD.write() {
        *g = Some(std::sync::Arc::downgrade(&arc));
    }
    std::sync::Arc::into_raw(arc).cast_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_service_coord_free(h: *mut NzbgetRsServiceCoord) {
    if !h.is_null() {
        crate::log::log_active_bytes(3, b"Destroying ServiceCoordinator");
        let arc = std::sync::Arc::from_raw(h.cast_const());
        let wk = std::sync::Arc::downgrade(&arc);
        if let Ok(mut g) = ACTIVE_SERVICE_COORD.write() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
            {
                *g = None;
            }
        }
        drop(arc);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_service_coord_ctl(
    h: *mut NzbgetRsServiceCoord,
    op: c_int,
    ctx: *mut c_void,
    cb: Option<NzbgetRsServiceCoordFn>,
) {
    let Some(sc) = h.as_ref() else {
        return;
    };
    match op {
        0 => {
            if let Ok(mut srvs) = sc.services.lock() {
                srvs.push(ServiceSlot {
                    ptr: ctx as usize,
                    last_work: 0,
                });
            }
        }
        4 => {
            if let Ok(mut srvs) = sc.services.lock() {
                srvs.push(ServiceSlot {
                    ptr: 0,
                    last_work: 0,
                });
            }
            sc.disk_service_attached.store(true, Ordering::Release);
        }
        1 => {
            crate::log::log_active_bytes(3, b"Waking up ServiceCoordinator");
            if let Ok(mut woken) = sc.wait_mutex.lock() {
                *woken = true;
                sc.wait_cond.notify_all();
            }
        }
        2 => {
            if let Ok(_guard) = sc.wait_mutex.lock() {
                sc.wait_cond.notify_all();
            }
        }
        3 => {
            let Some(host_cb) = cb else {
                return;
            };
            crate::log::log_active_bytes(3, b"Entering ServiceCoordinator-loop");
            while host_cb(ctx, std::ptr::null_mut(), 0) == 0 {
                let count = sc.services.lock().map_or(0, |g| g.len());
                for idx in 0..count {
                    let Some((svc_ptr, last_work)) = sc
                        .services
                        .lock()
                        .ok()
                        .and_then(|g| g.get(idx).map(|s| (s.ptr as *mut c_void, s.last_work)))
                    else {
                        continue;
                    };
                    let service_interval = if svc_ptr.is_null() {
                        crate::lifecycle::disk_service_interval_active()
                    } else {
                        host_cb(ctx, svc_ptr, 1)
                    };
                    if service_interval >= 0
                        && (service_coord_now() - last_work).abs() >= i64::from(service_interval)
                    {
                        if svc_ptr.is_null() {
                            crate::lifecycle::disk_service_work_active();
                        } else {
                            host_cb(ctx, svc_ptr, 2);
                        }
                        let done_at = service_coord_now();
                        if let Ok(mut g) = sc.services.lock() {
                            if let Some(slot) = g.get_mut(idx) {
                                slot.last_work = done_at;
                            }
                        }
                    }
                }

                let mut wait_interval: i32 = 60 * 60 * 24;
                let cur_time = service_coord_now();
                let count2 = sc.services.lock().map_or(0, |g| g.len());
                let debug_on = crate::log::active_debug_build();
                for idx in 0..count2 {
                    let Some((svc_ptr, last_work)) = sc
                        .services
                        .lock()
                        .ok()
                        .and_then(|g| g.get(idx).map(|s| (s.ptr as *mut c_void, s.last_work)))
                    else {
                        continue;
                    };
                    let service_interval = if svc_ptr.is_null() {
                        crate::lifecycle::disk_service_interval_active()
                    } else {
                        host_cb(ctx, svc_ptr, 1)
                    };
                    let mut remaining: i32 = -1;
                    if service_interval >= 0 {
                        let elapsed = (cur_time - last_work) as i32;
                        remaining = service_interval.min(service_interval.wrapping_sub(elapsed));
                        wait_interval = wait_interval.min(remaining);
                    }
                    if debug_on {
                        let msg =
                            format!("serviceInterval: {service_interval}, remaining: {remaining}");
                        crate::log::log_active_bytes(3, msg.as_bytes());
                    }
                }

                if debug_on {
                    let msg = format!("Waiting in ServiceCoordinator: {wait_interval}");
                    crate::log::log_active_bytes(3, msg.as_bytes());
                }
                if wait_interval > 0 {
                    if let Ok(mut guard) = sc.wait_mutex.lock() {
                        if !*guard && host_cb(ctx, std::ptr::null_mut(), 0) == 0 {
                            let dur =
                                std::time::Duration::from_millis((wait_interval as u64) * 1000);
                            if let Ok((new_guard, _)) =
                                sc.wait_cond.wait_timeout_while(guard, dur, |woken| {
                                    !*woken && unsafe { host_cb(ctx, std::ptr::null_mut(), 0) } == 0
                                })
                            {
                                guard = new_guard;
                            } else {
                                break;
                            }
                        }
                        *guard = false;
                    }
                }
            }
            crate::log::log_active_bytes(3, b"Exiting ServiceCoordinator-loop");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        // 2026-10-10 12:34:56 UTC, a Saturday
        let t = 1_791_635_696;
        let tm = gmtime(t);
        assert_eq!(
            (tm.year, tm.mon, tm.mday, tm.hour, tm.min, tm.sec, tm.wday),
            (2026, 9, 10, 12, 34, 56, 6)
        );
        assert_eq!(timegm(&tm), t);
        assert_eq!(gmtime(0).wday, 4);
        assert_eq!(gmtime(-1).year, 1969);
    }

    #[test]
    fn due() {
        let now = 1_791_635_696; // Saturday 12:34:56
        let mut tasks = [
            Task {
                hours: 12,
                minutes: 30,
                week_days: 0,
                last_executed: 0,
            },
            Task {
                hours: 12,
                minutes: 40,
                week_days: 0,
                last_executed: 0,
            },
            Task {
                hours: STARTUP_TASK,
                minutes: 0,
                week_days: 0,
                last_executed: 0,
            },
        ];
        let mut last = now - 600;
        let c = check_tasks(&mut tasks, &mut last, now, 0, 0, gmtime);
        assert_eq!(c.due, vec![0, 2]);
        assert!(!c.reset);
        assert_eq!(last, now);
    }

    #[test]
    fn scheduler_exec_startup_health_and_autoresume() {
        struct MockCtx {
            now: i64,
            resume_time: i64,
            speed_limit: i32,
            autoresumed: bool,
            emitted_tasks: Vec<[i32; 4]>,
        }
        unsafe extern "C" fn mock_action(
            raw: *mut c_void,
            sub: c_int,
            a1: c_int,
            a2: c_int,
            _argv: *const *const c_char,
            _argc: usize,
            ints: *mut c_int,
            i64s: *mut i64,
            _strs: *mut *const c_char,
        ) -> c_int {
            let ctx = &mut *(raw as *mut MockCtx);
            match sub {
                0 => {
                    let s = std::slice::from_raw_parts_mut(ints, 6);
                    s[0] = 0;
                    s[1] = 0;
                    s[2] = 0;
                    s[3] = 0;
                    s[4] = 0;
                    s[5] = 0;
                    *i64s = ctx.resume_time;
                    1
                }
                1 => {
                    if a1 == 0 {
                        ctx.speed_limit = a2;
                    } else if a1 == 4 {
                        ctx.autoresumed = true;
                        ctx.resume_time = 0;
                    }
                    1
                }
                2 => 0,
                5 => {
                    let v = std::slice::from_raw_parts(ints, 4);
                    ctx.emitted_tasks.push([v[0], v[1], v[2], v[3]]);
                    1
                }
                6 => {
                    *i64s = ctx.now;
                    1
                }
                _ => 0,
            }
        }

        unsafe {
            let h = nzbget_rs_scheduler_new();
            assert!(!h.is_null());
            assert_eq!(nzbget_rs_scheduler_exec(h, 1, std::ptr::null()), 1);

            let mut m = MockCtx {
                now: 1_791_635_696,
                resume_time: 1_791_635_600,
                speed_limit: 0,
                autoresumed: false,
                emitted_tasks: Vec::new(),
            };
            let param = b"512\0";
            let mut env = NzbgetRsSchedulerEnv {
                ctx: (&mut m as *mut MockCtx).cast(),
                a1: 7,
                a2: -1,
                a3: 0,
                a4: 0,
                a5: 4,
                s1: param.as_ptr() as *const c_char,
                log_cb: None,
                action_cb: Some(mock_action),
            };
            assert_eq!(nzbget_rs_scheduler_exec(h, 0, &env), 1);
            env.a1 = 8;
            env.a2 = 12;
            env.a3 = 40;
            env.a4 = 0x7f;
            env.a5 = 0;
            env.s1 = std::ptr::null();
            assert_eq!(nzbget_rs_scheduler_exec(h, 0, &env), 1);

            let tasks_out: Vec<[i32; 4]> = (*h)
                .collect_health_tasks()
                .into_iter()
                .map(|t| [t.id, t.hours, t.minutes, t.week_days_bits])
                .collect();
            assert_eq!(tasks_out, vec![[7, -1, 0, 0], [8, 12, 40, 0x7f]]);

            env.a5 = 0;
            assert_eq!(nzbget_rs_scheduler_exec(h, 2, &env), 1);
            assert_eq!(m.speed_limit, 512 * 1024);
            assert!(!m.autoresumed);

            m.now += 10;
            assert_eq!(nzbget_rs_scheduler_exec(h, 2, &env), 1);
            assert!(m.autoresumed);
            assert!(nzbget_rs_scheduler_exec(h, 1, std::ptr::null()) > 0);

            nzbget_rs_scheduler_free(h);
        }
    }
}
