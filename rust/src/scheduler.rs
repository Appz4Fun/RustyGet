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
#[inline]
pub fn scheduler_matches_server_ref(
    id: i32,
    server_id: i32,
    server_name: &[u8],
    server_ref: &[u8],
) -> bool {
    (id > 0 && server_id == id) || server_name.eq_ignore_ascii_case(server_ref)
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
        6 => b"Execute process\0",
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

use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

/// Level of a scheduler log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SchedLevel {
    Debug,
    Info,
    Error,
}

/// The work state the scheduler reads.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SchedState {
    pub local_time_offset: c_int,
    pub last_check_offset: c_int,
    pub speed_limit: c_int,
    pub pause_download: bool,
    pub pause_post_process: bool,
    pub pause_scan: bool,
    pub resume_time: i64,
}

/// A change the scheduler makes to the work state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SchedEdit {
    SpeedLimit(c_int),
    PauseDownload(bool),
    PausePostProcess(bool),
    PauseScan(bool),
    /// Clears the resume time and all pause flags.
    Autoresume,
}

/// What the scheduler acts on. Each default does nothing and reports no
/// data, so a test host implements only what it needs.
pub(crate) trait SchedulerHost {
    fn log(&self, _level: SchedLevel, _msg: &[u8]) {}
    /// The work state, or `None` while the queue is not loaded.
    fn state(&self) -> Option<SchedState> {
        None
    }
    fn edit(&self, _edit: SchedEdit) {}
    fn fetch_feeds(&self, _feed_list: &[u8]) {}
    fn start_process(&self, _task_id: c_int, _words: &[&[u8]]) {}
    /// The service clock, or 0 to use the system time.
    fn now(&self) -> i64 {
        0
    }
}

/// The bytes before the first NUL, as a C string would carry them.
fn until_nul(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

/// C `atoi` of `bytes` up to the first NUL.
fn c_atoi(bytes: &[u8]) -> c_int {
    let mut buf = until_nul(bytes).to_vec();
    buf.push(0);
    // SAFETY: `buf` is NUL-terminated.
    unsafe { libc::atoi(buf.as_ptr().cast()) }
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

fn current_unix_time(host: &dyn SchedulerHost) -> i64 {
    let now = host.now();
    if now != 0 {
        return now;
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

    pub(crate) fn add_task(
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

    pub(crate) fn service_interval_val(&self) -> i32 {
        self.service_interval.load(Ordering::SeqCst)
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

    fn execute_task(
        ws: &mut SchedulerWorkState,
        task: &SchedTaskEntry,
        is_debug: bool,
        host: &dyn SchedulerHost,
    ) {
        if is_debug {
            let cmd_bytes = scheduler_command_name(task.command);
            let cmd_slice = &cmd_bytes[..cmd_bytes.len().saturating_sub(1)];
            let mut msg = b"Executing scheduled command: ".to_vec();
            msg.extend_from_slice(cmd_slice);
            sched_log(host, SchedLevel::Debug, &msg);
        }

        let exec_proc =
            scheduler_should_execute_process(ws.execute_process, task.hours, STARTUP_TASK);

        match task.command {
            4 => {
                // scDownloadRate
                if !task.param.is_empty() {
                    let limit = scheduler_calc_speed_limit_bytes(c_atoi(&task.param));
                    host.edit(SchedEdit::SpeedLimit(limit));
                    ws.download_rate_changed = true;
                }
            }
            0 | 1 => {
                // scPauseDownload | scUnpauseDownload
                host.edit(SchedEdit::PauseDownload(task.command == 0));
                ws.pause_download_changed = true;
            }
            2 | 3 => {
                // scPausePostProcess | scUnpausePostProcess
                host.edit(SchedEdit::PausePostProcess(task.command == 2));
                ws.pause_post_process_changed = true;
            }
            7 | 8 => {
                // scPauseScan | scUnpauseScan
                host.edit(SchedEdit::PauseScan(task.command == 7));
                ws.pause_scan_changed = true;
            }
            6 => {
                // scProcess
                if exec_proc {
                    let words = crate::util::split_command_line(&task.param);
                    if words.is_empty() {
                        let mut err = b"Could not execute scheduled process-script, failed to parse command line: ".to_vec();
                        err.extend_from_slice(&task.param);
                        sched_log(host, SchedLevel::Error, &err);
                    } else {
                        let words: Vec<&[u8]> = words.iter().map(|w| until_nul(w)).collect();
                        host.start_process(task.id, &words);
                    }
                }
            }
            9 | 10 => {
                // scActivateServer | scDeactivateServer
                Self::edit_server(ws, task.command == 9, &task.param);
            }
            11 => {
                // scFetchFeed
                if exec_proc {
                    host.fetch_feeds(until_nul(&task.param));
                }
            }
            _ => {}
        }
    }

    fn edit_server(ws: &mut SchedulerWorkState, active: bool, server_list: &[u8]) {
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
                let tok = until_nul(&server_list[pos + tok_start..pos + tok_end]);
                pos += next_pos;
                let ref_id = c_atoi(tok);

                for s in &srvs {
                    if scheduler_matches_server_ref(ref_id, s.id, s.name.to_bytes(), tok) {
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
        }
        #[cfg(not(unix))]
        let _ = (ws, active, server_list);
    }

    fn check_tasks_and_log(
        &self,
        ws: &mut SchedulerWorkState,
        is_debug: bool,
        host: &dyn SchedulerHost,
    ) {
        ws.download_rate_changed = false;
        ws.pause_download_changed = false;
        ws.pause_post_process_changed = false;
        ws.pause_scan_changed = false;
        ws.server_changed = false;

        {
            let mut task_guard = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
            let current = current_unix_time(host);
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
                let st = host.state().unwrap_or_default();
                (i64::from(st.local_time_offset), i64::from(st.last_check_offset))
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
                sched_log(
                    host,
                    SchedLevel::Debug,
                    b"Reset scheduled tasks (detected clock change greater than 90 minutes or negative)",
                );
                ws.execute_process = false;
            }
            for (i, st) in sched_tasks.iter().enumerate() {
                task_guard[i].last_executed = st.last_executed;
            }
            for &idx in &res.due {
                Self::execute_task(ws, &task_guard[idx], is_debug, host);
            }
            ws.last_check = last_check;
        }

        // PrintLog
        if ws.download_rate_changed
            || ws.pause_download_changed
            || ws.pause_post_process_changed
            || ws.pause_scan_changed
        {
            let st = host.state().unwrap_or_default();
            if ws.download_rate_changed {
                let msg = format!(
                    "Scheduler: setting download rate to {} KB/s",
                    scheduler_bytes_to_kb(st.speed_limit)
                );
                sched_log(host, SchedLevel::Info, msg.as_bytes());
            }
            if ws.pause_download_changed {
                let act = scheduler_pause_action_str(st.pause_download);
                let mut msg = b"Scheduler: ".to_vec();
                msg.extend_from_slice(&act[..act.len() - 1]);
                msg.extend_from_slice(b" download");
                sched_log(host, SchedLevel::Info, &msg);
            }
            if ws.pause_post_process_changed {
                let act = scheduler_pause_action_str(st.pause_post_process);
                let mut msg = b"Scheduler: ".to_vec();
                msg.extend_from_slice(&act[..act.len() - 1]);
                msg.extend_from_slice(b" post-processing");
                sched_log(host, SchedLevel::Info, &msg);
            }
            if ws.pause_scan_changed {
                let act = scheduler_pause_action_str(st.pause_scan);
                let mut msg = b"Scheduler: ".to_vec();
                msg.extend_from_slice(&act[..act.len() - 1]);
                msg.extend_from_slice(b" scan");
                sched_log(host, SchedLevel::Info, &msg);
            }
        }
        #[cfg(unix)]
        if ws.server_changed {
            if let Some(pool) = crate::net::active_server_pool() {
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
                            sched_log(host, SchedLevel::Info, &msg);
                        }
                    }
                }
                pool.changed();
            }
        }
    }

    /// `Scheduler::ServiceWork`.
    pub(crate) fn service_work(&self, host: &dyn SchedulerHost, is_debug: bool) {
        sched_log(host, SchedLevel::Debug, b"Scheduler service work");

        if host.state().is_none() {
            return;
        }

        sched_log(host, SchedLevel::Debug, b"Scheduler service work: doing work");

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
            self.check_tasks_and_log(&mut ws, is_debug, host);
            ws.first_checked = true;
            return;
        }

        ws.execute_process = true;
        self.check_tasks_and_log(&mut ws, is_debug, host);

        // CheckScheduledResume
        let resume_time = host.state().unwrap_or_default().resume_time;
        let now_resume = current_unix_time(host);
        if scheduler_should_autoresume(resume_time, now_resume) {
            sched_log(host, SchedLevel::Info, b"Autoresume");
            host.edit(SchedEdit::Autoresume);
        }

        // ScheduleNextWork
        let now_next = current_unix_time(host);
        self.service_interval.store(
            scheduler_calc_next_work_interval(now_next),
            Ordering::SeqCst,
        );
    }

    #[cfg(unix)]
    pub(crate) fn service_work_active(&self) {
        self.service_work(&ActiveSchedulerHost, crate::log::active_debug_build());
    }
}

fn sched_log(host: &dyn SchedulerHost, level: SchedLevel, msg: &[u8]) {
    host.log(level, until_nul(msg));
}

/// The daemon's work state, feed coordinator and process runner.
#[cfg(unix)]
struct ActiveSchedulerHost;

#[cfg(unix)]
impl SchedulerHost for ActiveSchedulerHost {
    fn log(&self, level: SchedLevel, msg: &[u8]) {
        let kind = match level {
            SchedLevel::Debug => 3,
            SchedLevel::Info => 0,
            SchedLevel::Error => 1,
        };
        crate::log::log_active_bytes(kind, msg);
    }

    fn state(&self) -> Option<SchedState> {
        if !crate::ffi::is_queue_loaded() {
            return None;
        }
        let ws = crate::ffi::active_workstate()?;
        let lto = ws.local_time_offset();
        Some(SchedState {
            local_time_offset: lto,
            last_check_offset: lto,
            speed_limit: ws.speed_limit(),
            pause_download: ws.pause_download(),
            pause_post_process: ws.pause_post_process(),
            pause_scan: ws.pause_scan(),
            resume_time: ws.resume_time(),
        })
    }

    fn edit(&self, edit: SchedEdit) {
        if let Some(ws) = crate::ffi::active_workstate() {
            match edit {
                SchedEdit::SpeedLimit(limit) => ws.set_speed_limit(limit),
                SchedEdit::PauseDownload(paused) => ws.set_pause_download(paused),
                SchedEdit::PausePostProcess(paused) => ws.set_pause_post_process(paused),
                SchedEdit::PauseScan(paused) => ws.set_pause_scan(paused),
                SchedEdit::Autoresume => {
                    ws.set_resume_time(0);
                    ws.set_pause_download(false);
                    ws.set_pause_post_process(false);
                    ws.set_pause_scan(false);
                }
            }
        }
    }

    fn fetch_feeds(&self, feed_list: &[u8]) {
        if let Some(fc) = crate::feed::active_feed_coord() {
            fc.fetch_feed_list_bytes(feed_list);
        }
    }

    fn start_process(&self, task_id: c_int, words: &[&[u8]]) {
        crate::procrun::scheduler_process_start_active(task_id, words);
    }

    fn now(&self) -> i64 {
        service_coord_now()
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
static DAEMON_SCHEDULER: std::sync::RwLock<Option<std::sync::Arc<NzbgetRsScheduler>>> =
    std::sync::RwLock::new(None);

pub(crate) fn active_scheduler() -> Option<std::sync::Arc<NzbgetRsScheduler>> {
    ACTIVE_SCHEDULER
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

pub(crate) fn create_daemon_scheduler() -> std::sync::Arc<NzbgetRsScheduler> {
    let arc = std::sync::Arc::new(NzbgetRsScheduler::new());
    if let Ok(mut g) = ACTIVE_SCHEDULER.write() {
        *g = Some(std::sync::Arc::downgrade(&arc));
    }
    if let Ok(mut dg) = DAEMON_SCHEDULER.write() {
        *dg = Some(std::sync::Arc::clone(&arc));
    }
    arc
}

pub(crate) fn shutdown_daemon_scheduler() {
    let prev = DAEMON_SCHEDULER.write().ok().and_then(|mut g| g.take());
    if let Some(arc) = prev {
        let wk = std::sync::Arc::downgrade(&arc);
        if let Ok(mut g) = ACTIVE_SCHEDULER.write() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
            {
                *g = None;
            }
        }
    }
}

struct ServiceSlot {
    kind: u8,
    last_work: i64,
}

pub struct NzbgetRsServiceCoord {
    services: Mutex<Vec<ServiceSlot>>,
    wait_mutex: Mutex<bool>,
    wait_cond: std::sync::Condvar,
    disk_service_attached: AtomicBool,
    #[cfg(unix)]
    daemon_stopped: AtomicBool,
    #[cfg(unix)]
    daemon_running: AtomicBool,
}

static ACTIVE_SERVICE_COORD: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsServiceCoord>>> =
    std::sync::RwLock::new(None);
#[cfg(unix)]
static DAEMON_SERVICE_COORD: std::sync::RwLock<Option<std::sync::Arc<NzbgetRsServiceCoord>>> =
    std::sync::RwLock::new(None);
#[cfg(unix)]
static DAEMON_SERVICE_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

#[cfg(unix)]
pub(crate) fn wake_active_service_coord() {
    let Some(sc) = ACTIVE_SERVICE_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
    else {
        return;
    };
    crate::log::log_active_bytes(3, b"Waking up ServiceCoordinator");
    if let Ok(mut woken) = sc.wait_mutex.lock() {
        *woken = true;
        sc.wait_cond.notify_all();
    };
}

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

#[cfg(unix)]
fn daemon_service_interval(kind: u8) -> i32 {
    match kind {
        0 => crate::lifecycle::disk_service_interval_active(),
        1 => active_scheduler().map_or(-1, |s| s.service_interval_val()),
        2 => crate::twin::service_interval_active(),
        3 => 60 * 60,
        4 => crate::scanner::active_scanner().map_or(-1, |s| s.service_interval_active()),
        _ => -1,
    }
}

#[cfg(unix)]
fn daemon_service_work(kind: u8) {
    match kind {
        0 => crate::lifecycle::disk_service_work_active(),
        1 => {
            if let Some(s) = active_scheduler() {
                s.service_work_active();
            }
        }
        2 => crate::twin::service_work_active(),
        3 => crate::historycoord::service_work_active(),
        4 => {
            if let Some(s) = crate::scanner::active_scanner() {
                s.service_work_active();
            }
        }
        _ => {}
    }
}

#[cfg(unix)]
impl NzbgetRsServiceCoord {
    fn run_loop(&self) {
        crate::log::log_active_bytes(3, b"Entering ServiceCoordinator-loop");
        while !self.daemon_stopped.load(Ordering::SeqCst) {
            let count = self.services.lock().map_or(0, |g| g.len());
            for idx in 0..count {
                let Some((kind, last_work)) = self
                    .services
                    .lock()
                    .ok()
                    .and_then(|g| g.get(idx).map(|s| (s.kind, s.last_work)))
                else {
                    continue;
                };
                let service_interval = daemon_service_interval(kind);
                if service_interval >= 0
                    && (service_coord_now() - last_work).abs() >= i64::from(service_interval)
                {
                    daemon_service_work(kind);
                    let done_at = service_coord_now();
                    if let Ok(mut g) = self.services.lock() {
                        if let Some(slot) = g.get_mut(idx) {
                            slot.last_work = done_at;
                        }
                    }
                }
            }

            let mut wait_interval: i32 = 60 * 60 * 24;
            let cur_time = service_coord_now();
            let count2 = self.services.lock().map_or(0, |g| g.len());
            let debug_on = crate::log::active_debug_build();
            for idx in 0..count2 {
                let Some((kind, last_work)) = self
                    .services
                    .lock()
                    .ok()
                    .and_then(|g| g.get(idx).map(|s| (s.kind, s.last_work)))
                else {
                    continue;
                };
                let service_interval = daemon_service_interval(kind);
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
                if let Ok(mut guard) = self.wait_mutex.lock() {
                    if !*guard && !self.daemon_stopped.load(Ordering::SeqCst) {
                        let dur = std::time::Duration::from_millis((wait_interval as u64) * 1000);
                        if let Ok((new_guard, _)) =
                            self.wait_cond.wait_timeout_while(guard, dur, |woken| {
                                !*woken && !self.daemon_stopped.load(Ordering::SeqCst)
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
}

#[cfg(unix)]
pub(crate) fn create_daemon_service_coord() {
    crate::log::log_active_bytes(3, b"Creating ServiceCoordinator");
    let arc = std::sync::Arc::new(NzbgetRsServiceCoord {
        services: Mutex::new(vec![
            ServiceSlot {
                kind: 4,
                last_work: 0,
            },
            ServiceSlot {
                kind: 3,
                last_work: 0,
            },
            ServiceSlot {
                kind: 2,
                last_work: 0,
            },
            ServiceSlot {
                kind: 1,
                last_work: 0,
            },
            ServiceSlot {
                kind: 0,
                last_work: 0,
            },
        ]),
        wait_mutex: Mutex::new(false),
        wait_cond: std::sync::Condvar::new(),
        disk_service_attached: AtomicBool::new(true),
        daemon_stopped: AtomicBool::new(false),
        daemon_running: AtomicBool::new(false),
    });
    if let Ok(mut g) = ACTIVE_SERVICE_COORD.write() {
        *g = Some(std::sync::Arc::downgrade(&arc));
    }
    if let Ok(mut g) = DAEMON_SERVICE_COORD.write() {
        *g = Some(arc);
    }
    crate::scanner::create_daemon_scanner();
    crate::twin::create_daemon_twincheck();
    crate::dupesearch_coord::create_daemon_dupesearch();
    crate::urlcoord::create_daemon_url_coord();
    crate::feed::create_daemon_feed_coord();
    crate::articledl::create_daemon_memory_monitor();
    crate::statmeter::create_daemon_statmeter();
    crate::maintenance::create_daemon_maintenance();
    let _ = create_daemon_scheduler();
}

#[cfg(unix)]
pub(crate) fn start_daemon_service_coord() {
    let Some(sc) = ACTIVE_SERVICE_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
    else {
        return;
    };
    let mut g = DAEMON_SERVICE_HANDLE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if g.is_some() {
        return;
    }
    sc.daemon_stopped.store(false, Ordering::SeqCst);
    sc.daemon_running.store(true, Ordering::SeqCst);
    let sc_run = std::sync::Arc::clone(&sc);
    if let Ok(h) = std::thread::Builder::new()
        .name("service-coord".into())
        .spawn(move || {
            struct RunningReset(std::sync::Arc<NzbgetRsServiceCoord>);
            impl Drop for RunningReset {
                fn drop(&mut self) {
                    self.0.daemon_running.store(false, Ordering::SeqCst);
                }
            }
            let _reset = RunningReset(std::sync::Arc::clone(&sc_run));
            let _tg = crate::ffi::RustThreadGuard::enter(&sc_run.daemon_stopped);
            sc_run.run_loop();
        })
    {
        *g = Some(h);
    } else {
        sc.daemon_running.store(false, Ordering::SeqCst);
    }
}

#[cfg(unix)]
pub(crate) fn stop_daemon_service_coord() {
    if let Some(sc) = ACTIVE_SERVICE_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
    {
        sc.daemon_stopped.store(true, Ordering::SeqCst);
        if let Ok(_guard) = sc.wait_mutex.lock() {
            sc.wait_cond.notify_all();
        }
    }
}

#[cfg(unix)]
pub(crate) fn is_daemon_service_coord_running() -> bool {
    ACTIVE_SERVICE_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
        .is_some_and(|sc| sc.daemon_running.load(Ordering::SeqCst))
}

#[cfg(unix)]
pub(crate) fn shutdown_daemon_service_coord() {
    stop_daemon_service_coord();
    let handle = DAEMON_SERVICE_HANDLE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(h) = handle {
        let _ = h.join();
    }
    if DAEMON_SERVICE_COORD
        .write()
        .ok()
        .and_then(|mut g| g.take())
        .is_some()
    {
        crate::log::log_active_bytes(3, b"Destroying ServiceCoordinator");
    }
    if let Ok(mut g) = ACTIVE_SERVICE_COORD.write() {
        *g = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_leap_correction_makes_one_task_due_many_times() {
        // The large-correction TZif fixture in scheduler_differential.py,
        // expressed here independently of the host's libc timezone support.
        let calendar = |time: i64| {
            let correction = if time < 78_796_800 {
                0
            } else {
                1 + ((time - 78_796_800) / 86400).min(14) * 86400
            };
            gmtime(time - correction)
        };
        let mut tasks = [Task {
            hours: 336,
            minutes: 0,
            week_days: 0,
            last_executed: 0,
        }];
        let mut last = 0;
        let r = check_tasks(&mut tasks, &mut last, 80_049_600, 0, 0, calendar);
        assert_eq!(r.due, [0; 15]);
        assert!(r.reset);
        assert_eq!(tasks[0].last_executed, 80_049_599);
        assert_eq!(last, 80_049_600);
    }

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
    fn scheduler_service_work_startup_health_and_autoresume() {
        use std::cell::Cell;
        #[derive(Default)]
        struct MockHost {
            now: Cell<i64>,
            resume_time: Cell<i64>,
            speed_limit: Cell<i32>,
            autoresumed: Cell<bool>,
        }
        impl SchedulerHost for MockHost {
            fn state(&self) -> Option<SchedState> {
                Some(SchedState {
                    resume_time: self.resume_time.get(),
                    ..SchedState::default()
                })
            }
            fn edit(&self, edit: SchedEdit) {
                match edit {
                    SchedEdit::SpeedLimit(limit) => self.speed_limit.set(limit),
                    SchedEdit::Autoresume => {
                        self.autoresumed.set(true);
                        self.resume_time.set(0);
                    }
                    _ => {}
                }
            }
            fn now(&self) -> i64 {
                self.now.get()
            }
        }

        let sched = NzbgetRsScheduler::new();
        assert_eq!(sched.service_interval_val(), 1);

        let m = MockHost::default();
        m.now.set(1_791_635_696);
        m.resume_time.set(1_791_635_600);
        sched.add_task(7, -1, 0, 0, 4, b"512");
        sched.add_task(8, 12, 40, 0x7f, 0, b"");

        let tasks_out: Vec<[i32; 4]> = sched
            .collect_health_tasks()
            .into_iter()
            .map(|t| [t.id, t.hours, t.minutes, t.week_days_bits])
            .collect();
        assert_eq!(tasks_out, vec![[7, -1, 0, 0], [8, 12, 40, 0x7f]]);

        sched.service_work(&m, false);
        assert_eq!(m.speed_limit.get(), 512 * 1024);
        assert!(!m.autoresumed.get());

        m.now.set(m.now.get() + 10);
        sched.service_work(&m, false);
        assert!(m.autoresumed.get());
        assert!(sched.service_interval_val() > 0);
    }
}
