//! ServerVolume's slot arithmetic (StatMeter.cpp): which second, minute,
//! hour and day slot a local time falls in (CalcSlots), and AddStats'
//! clearing of the slots the clock moved past (both ways) before adding the
//! bytes. Times are cut to a C int and divided as C ints, as the C++ did; the
//! C++ indexed the arrays with negative slots for times before 1970 or after
//! 2038-01-19 (memory corruption): those writes are skipped here.

const DAYS_UP_TO_2013_JAN_1: i32 = 15706;
const DAYS_IN_TWENTY_YEARS: i32 = 366 * 20;

/// NzbgetRsVolumeSlots
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slots {
    pub sec: i32,
    pub min: i32,
    pub hour: i32,
    /// -1 outside the 20 years from 2013 (no day counters)
    pub day: i32,
    /// whether the day is in those 20 years (the day arrays may grow)
    pub in_range: i32,
}

/// CalcSlots: the slots of `loc_cur_time`; sets `first_day` on the first day
/// counted (or an earlier one).
pub fn calc_slots(loc_cur_time: i64, first_day: &mut i32) -> Slots {
    let t = loc_cur_time as i32;
    let days = t / 86400;
    let mut s = Slots {
        sec: t % 60,
        min: (t / 60) % 60,
        hour: (t % 86400) / 3600,
        day: days - DAYS_UP_TO_2013_JAN_1 + 1,
        in_range: 0,
    };
    if 0 <= s.day && s.day < DAYS_IN_TWENTY_YEARS {
        if *first_day == 0 || *first_day > days {
            *first_day = days;
        }
        s.day = days.wrapping_sub(*first_day);
        s.in_range = 1;
    } else {
        s.day = -1;
    }
    s
}

/// Zeroes `count` slots back (or forward) from `slot` in a ring of `ring`
/// slots, as the C++ loops did.
fn clear(array: &mut [i64], slot: i32, count: i32, sign: i32, ring: i32) {
    for i in 0..count.max(0) {
        let mut nul = slot.wrapping_sub(i.wrapping_mul(sign));
        if nul < 0 {
            nul += ring;
        }
        if nul >= ring {
            nul -= ring;
        }
        if let Some(v) = usize::try_from(nul).ok().and_then(|n| array.get_mut(n)) {
            *v = 0;
        }
    }
}

fn add(array: &mut [i64], slot: i32, bytes: i64) {
    if let Some(v) = usize::try_from(slot).ok().and_then(|n| array.get_mut(n)) {
        *v = v.wrapping_add(bytes);
    }
}

/// AddStats for the second, minute and hour counters: clears the slots passed
/// since `loc_data_time` (the previous update) and adds `bytes`.
#[allow(clippy::too_many_arguments)]
pub fn add_stats(
    seconds: &mut [i64],
    minutes: &mut [i64],
    hours: &mut [i64],
    s: &Slots,
    last_min_slot: i32,
    last_hour_slot: i32,
    loc_cur_time: i64,
    loc_data_time: i64,
    bytes: i64,
) {
    if loc_cur_time != loc_data_time {
        let mut total_delta = loc_cur_time.wrapping_sub(loc_data_time) as i32;
        let sign = if total_delta >= 0 { 1 } else { -1 };
        total_delta = total_delta.wrapping_abs();

        let mut sec_delta = total_delta;
        if sign < 0 {
            sec_delta = sec_delta.wrapping_add(1);
        }
        if sec_delta >= 60 {
            sec_delta = 60;
        }
        clear(seconds, s.sec, sec_delta, sign, 60);

        let mut min_delta = total_delta / 60;
        if sign < 0 {
            min_delta += 1;
        }
        if min_delta.wrapping_abs() >= 60 {
            min_delta = 60;
        }
        if min_delta == 0 && s.min != last_min_slot {
            min_delta = 1;
        }
        clear(minutes, s.min, min_delta, sign, 60);

        let mut hour_delta = total_delta / 3600;
        if sign < 0 {
            hour_delta += 1;
        }
        if hour_delta >= 24 {
            hour_delta = 24;
        }
        if hour_delta == 0 && s.hour != last_hour_slot {
            hour_delta = 1;
        }
        clear(hours, s.hour, hour_delta, sign, 24);
    }
    add(seconds, s.sec, bytes);
    add(minutes, s.min, bytes);
    add(hours, s.hour, bytes);
}

/// Accumulates day slot and total/custom volume stats in `ServerVolume::AddStats`.
/// Returns `(update_day, new_day_bytes, new_day_failed, new_day_success, new_total_bytes, new_custom_bytes)`.
#[allow(clippy::too_many_arguments)]
pub fn accum_day_and_total_volume(
    day_slot: i32,
    cur_day_bytes: i64,
    cur_day_failed: i32,
    cur_day_success: i32,
    cur_total_bytes: i64,
    cur_custom_bytes: i64,
    add_bytes: i64,
    add_failed: i32,
    add_success: i32,
) -> (bool, i64, i32, i32, i64, i64) {
    let new_total = cur_total_bytes.wrapping_add(add_bytes);
    let new_custom = cur_custom_bytes.wrapping_add(add_bytes);
    if day_slot >= 0 {
        (
            true,
            cur_day_bytes.wrapping_add(add_bytes),
            cur_day_failed.wrapping_add(add_failed),
            cur_day_success.wrapping_add(add_success),
            new_total,
            new_custom,
        )
    } else {
        (
            false,
            cur_day_bytes,
            cur_day_failed,
            cur_day_success,
            new_total,
            new_custom,
        )
    }
}

/// `StatMeter::AdjustTimeOffset`:
/// Computes `(local_time_delta, total_offset)` from `utc_time`, `loc_time`, and `time_correction`.
#[inline]
pub fn calc_local_time_offset(utc_time: i64, loc_time: i64, time_correction: i32) -> (i32, i32) {
    let delta = utc_time.wrapping_sub(loc_time) as i32;
    (delta, delta.wrapping_add(time_correction))
}

/// `StatMeter::EnterLeaveStandBy`:
/// Computes the updated `m_startDownload` timestamp when leaving standby.
#[inline]
pub fn calc_resumed_download_start(
    prev_start_download: i64,
    cur_time: i64,
    paused_from: i64,
) -> i64 {
    prev_start_download.wrapping_add(cur_time.wrapping_sub(paused_from))
}

/// `StatMeter::CalcMonthSlots`:
/// Computes elapsed days when `QuotaStartDay > dayparts.tm_mday` using the previous month start timestamp.
#[inline]
pub fn calc_month_slots_prev_month(loc_cur_time: i64, prev_month_time: i64) -> i32 {
    let diff = loc_cur_time.wrapping_sub(prev_month_time) as i32;
    (diff / 60 / 60 / 24).wrapping_add(1)
}

/// `StatMeter::CalcMonthSlots`:
/// Computes elapsed days when `QuotaStartDay <= dayparts.tm_mday` within the current month.
#[inline]
pub fn calc_month_slots_same_month(tm_mday: i32, quota_start_day: i32) -> i32 {
    tm_mday.wrapping_sub(quota_start_day).wrapping_add(1)
}

pub(crate) fn days_to_ymd(z_days: i64) -> (i64, i32, i32) {
    let z = z_days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as i32, d as i32)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i64, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Computes `StatMeter::CalcMonthSlots` directly from `loc_cur_time` (seconds since epoch)
/// and `quota_start_day` (1..=31), matching the UTC civil-calendar month rollover and
/// short-previous-month clamp to the 1st of the current month.
pub fn calc_month_slots_utc(loc_cur_time: i64, quota_start_day: i32) -> i32 {
    let z_days = loc_cur_time.div_euclid(86_400);
    let (year, month, mday) = days_to_ymd(z_days);
    if quota_start_day > mday {
        let (prev_year, prev_month) = if month == 1 {
            (year - 1, 12)
        } else {
            (year, month - 1)
        };
        let prev_dim = days_in_month(prev_year, prev_month);
        if quota_start_day > prev_dim {
            mday
        } else {
            (prev_dim - quota_start_day) + mday + 1
        }
    } else {
        calc_month_slots_same_month(mday, quota_start_day)
    }
}

pub const SPEEDMETER_SLOTS: usize = 30;
pub const SPEEDMETER_SLOTSIZE: i32 = 1;

/// Evaluates `StatMeter::IntervalCheck` time-step and timezone refresh decisions.
/// Returns `(adjust_clocks, clock_delta, adjust_time_offset)`.
pub fn eval_interval_check(
    cur_time: i64,
    last_check: i64,
    last_time_offset: i64,
    start_download: i64,
    stand_by: bool,
) -> (bool, i64, bool, bool) {
    let diff = cur_time - last_check;
    if diff > 60 || diff < 0 {
        let delta = diff + 1;
        let adjust_download = start_download != 0 && !stand_by;
        (true, delta, adjust_download, true)
    } else if last_time_offset > cur_time
        || cur_time - last_time_offset > 60 * 60 * 3
        || (cur_time - last_time_offset > 60 && !stand_by)
    {
        (false, 0, false, true)
    } else {
        (false, 0, false, false)
    }
}

/// Computes `StatMeter::CalcTotalStat` (`up_time_sec`, `dn_time_sec`).
pub fn calc_total_stat(
    cur_time: i64,
    start_server: i64,
    start_download: i64,
    paused_from: i64,
    stand_by: bool,
) -> (i32, i32) {
    let up_time_sec = if start_server > 0 {
        (cur_time - start_server) as i32
    } else {
        0
    };
    let dn_time_sec = if stand_by {
        (paused_from - start_download) as i32
    } else {
        (cur_time - start_download) as i32
    };
    (up_time_sec, dn_time_sec)
}

/// Computes `StatMeter::CalcCurrentDownloadSpeed`.
pub fn calc_current_download_speed(
    stand_by: bool,
    cur_time: i64,
    speed_start_time: i32,
    speed_total_bytes: i64,
) -> i64 {
    if stand_by {
        return 0;
    }
    let time_diff = cur_time - (speed_start_time as i64) * (SPEEDMETER_SLOTSIZE as i64);
    if time_diff == 0 {
        return 0;
    }
    speed_total_bytes / time_diff
}

/// Computes `StatMeter::CalcMomentaryDownloadSpeed`.
pub fn calc_momentary_download_speed(cur_time: i64, cur_sec_time: i64, cur_sec_bytes: i64) -> i64 {
    if cur_time == cur_sec_time {
        cur_sec_bytes
    } else {
        0
    }
}

/// Performs `StatMeter::AddSpeedReading` ring-buffer slot advancement and byte accumulation.
#[allow(clippy::too_many_arguments)]
pub fn add_speed_reading(
    bytes: i32,
    cur_time: i64,
    cur_sec_time: &mut i64,
    cur_sec_bytes: &mut i64,
    speed_bytes: &mut [i32; SPEEDMETER_SLOTS],
    speed_time: &mut [i32; SPEEDMETER_SLOTS],
    speed_bytes_index: &mut i32,
    speed_total_bytes: &mut i64,
    speed_start_time: &mut i32,
    speed_correction: &mut i64,
    all_bytes: &mut i64,
) {
    let now_slot = (cur_time as i32) / SPEEDMETER_SLOTSIZE;

    if cur_time != *cur_sec_time {
        *cur_sec_time = cur_time;
        *cur_sec_bytes = 0;
    }
    *cur_sec_bytes += bytes as i64;

    let mut idx = (*speed_bytes_index).clamp(0, (SPEEDMETER_SLOTS - 1) as i32) as usize;
    while now_slot > speed_time[idx] {
        idx += 1;
        if idx >= SPEEDMETER_SLOTS {
            idx = 0;
        }
        *speed_total_bytes -= speed_bytes[idx] as i64;
        *speed_start_time = speed_time[idx];
        speed_bytes[idx] = 0;
        speed_time[idx] = now_slot;
    }
    *speed_bytes_index = idx as i32;

    if cur_time > *speed_correction {
        let mut total: i64 = 0;
        for &b in speed_bytes.iter() {
            total += b as i64;
        }
        *speed_total_bytes = total;
        *speed_correction = cur_time;
    }

    if *speed_total_bytes == 0 {
        *speed_start_time = now_slot;
    }
    speed_bytes[idx] = speed_bytes[idx].wrapping_add(bytes);
    *speed_total_bytes += bytes as i64;
    *all_bytes += bytes as i64;
}

/// Computes `StatMeter::CalcQuotaUsage` (`month_bytes`, `day_bytes`).
pub fn calc_quota_usage(
    loc_time: i64,
    first_day: i32,
    bytes_per_days: &[i64],
    elapsed_slots: i32,
) -> (i64, i64) {
    let day_slot = (loc_time / 86400) as i32 - first_day;
    let day_bytes = if day_slot >= 0 && (day_slot as usize) < bytes_per_days.len() {
        bytes_per_days[day_slot as usize]
    } else {
        0
    };

    let mut month_bytes: i64 = 0;
    let end_slot = (day_slot - elapsed_slots).max(-1);
    let mut slot = day_slot;
    while slot >= 0 && slot > end_slot {
        if (slot as usize) < bytes_per_days.len() {
            month_bytes += bytes_per_days[slot as usize];
        }
        slot -= 1;
    }
    (month_bytes, day_bytes)
}

/// Evaluates `StatMeter::CheckQuota` transition action and new quota-reached state.
/// Action codes:
/// - `0`: no log message
/// - `1`: warn `"Monthly quota reached at %s"`
/// - `2`: warn `"Daily quota reached at %s"`
/// - `3`: info `"Quota lifted"`
pub fn eval_quota_status(
    monthly_quota_mb: i32,
    daily_quota_mb: i32,
    month_bytes: i64,
    day_bytes: i64,
    was_quota_reached: bool,
) -> (bool, i32) {
    let monthly_reached =
        monthly_quota_mb > 0 && month_bytes >= (monthly_quota_mb as i64) * 1024 * 1024;
    let daily_reached = daily_quota_mb > 0 && day_bytes >= (daily_quota_mb as i64) * 1024 * 1024;
    let action = if monthly_reached && !was_quota_reached {
        1
    } else if daily_reached && !was_quota_reached {
        2
    } else if !monthly_reached && !daily_reached && was_quota_reached {
        3
    } else {
        0
    };
    (monthly_reached || daily_reached, action)
}

/// Returns `(day_slot + 1) as usize` when `day_slot + 1 > current_len`, or `0` when no resize is needed,
/// for `ServerVolume::CalcSlots`.
#[cfg(test)]
pub fn server_volume_required_day_len(day_slot: i32, current_len: usize) -> usize {
    let needed = (day_slot.saturating_add(1)).max(0) as usize;
    if needed > current_len {
        needed
    } else {
        0
    }
}

/// Formats `"[first_idx]=v0 [first_idx+1]=v1 ... "` for `ServerVolume::LogDebugInfo`.
pub fn statmeter_format_indexed_bytes_line(first_idx: i32, values: &[i64]) -> Vec<u8> {
    let mut out = String::with_capacity(values.len() * 12);
    for (i, &val) in values.iter().enumerate() {
        use std::fmt::Write;
        let _ = write!(out, "[{}]={val} ", first_idx.wrapping_add(i as i32));
    }
    out.into_bytes()
}

#[cfg(unix)]
mod runtime {
    use super::*;
    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
    use std::sync::Mutex;

    use crate::diskstate_format::{
        diskstate_exec, DiskAuxSinkFn, NzbgetRsDiskStateEnv, NzbgetRsDiskVolumeStatInput,
        NzbgetRsServerMatchItem,
    };
    use crate::log::NzbgetRsLog;
    use crate::options::NzbgetRsOptions;
    use crate::xmlrpc_format::{
        NzbgetRsRpcArticlePair, NzbgetRsRpcServerVolumeItem, NzbgetRsXmlRpcEmitFn,
    };

    #[inline]
    fn current_unix_time() -> i64 {
        unsafe { libc::time(ptr::null_mut()) as i64 }
    }

    #[derive(Clone)]
    struct RsServerVolume {
        bytes_per_seconds: Vec<i64>,
        bytes_per_minutes: Vec<i64>,
        bytes_per_hours: Vec<i64>,
        bytes_per_days: Vec<i64>,
        articles_per_days: Vec<NzbgetRsRpcArticlePair>,
        first_day: i32,
        total_bytes: i64,
        custom_bytes: i64,
        data_time: i64,
        custom_time: i64,
        counters_reset_time: i64,
        sec_slot: i32,
        min_slot: i32,
        hour_slot: i32,
        day_slot: i32,
    }

    impl RsServerVolume {
        fn new(now: i64) -> Self {
            Self {
                bytes_per_seconds: vec![0; 60],
                bytes_per_minutes: vec![0; 60],
                bytes_per_hours: vec![0; 24],
                bytes_per_days: Vec::new(),
                articles_per_days: Vec::new(),
                first_day: 0,
                total_bytes: 0,
                custom_bytes: 0,
                data_time: 0,
                custom_time: now,
                counters_reset_time: now,
                sec_slot: 0,
                min_slot: 0,
                hour_slot: 0,
                day_slot: 0,
            }
        }

        fn calc_slots(&mut self, loc_cur_time: i64) {
            let slots = calc_slots(loc_cur_time, &mut self.first_day);
            self.sec_slot = slots.sec;
            self.min_slot = slots.min;
            self.hour_slot = slots.hour;
            self.day_slot = slots.day;
            if slots.in_range != 0 {
                let needed = ((self.day_slot as i64) + 1).clamp(0, i32::MAX as i64) as usize;
                if self.bytes_per_days.len() < needed {
                    self.bytes_per_days.resize(needed, 0);
                }
                if self.articles_per_days.len() < needed {
                    self.articles_per_days.resize(
                        needed,
                        NzbgetRsRpcArticlePair {
                            failed: 0,
                            success: 0,
                        },
                    );
                }
            }
        }

        fn add_stats(&mut self, bytes: u32, failed: u32, success: u32, local_time_offset: i32) {
            let cur_time = current_unix_time();
            let off = i64::from(local_time_offset);
            let loc_cur_time = cur_time.wrapping_add(off);
            let loc_data_time = self.data_time.wrapping_add(off);
            let last_min = self.min_slot;
            let last_hour = self.hour_slot;
            self.calc_slots(loc_cur_time);
            let slots = Slots {
                sec: self.sec_slot,
                min: self.min_slot,
                hour: self.hour_slot,
                day: self.day_slot,
                in_range: 0,
            };
            let b = i64::from(bytes);
            add_stats(
                &mut self.bytes_per_seconds,
                &mut self.bytes_per_minutes,
                &mut self.bytes_per_hours,
                &slots,
                last_min,
                last_hour,
                loc_cur_time,
                loc_data_time,
                b,
            );
            if self.day_slot >= 0 {
                let idx = self.day_slot as usize;
                if idx < self.bytes_per_days.len() {
                    self.bytes_per_days[idx] = self.bytes_per_days[idx].wrapping_add(b);
                }
                if idx < self.articles_per_days.len() {
                    self.articles_per_days[idx].failed =
                        self.articles_per_days[idx].failed.wrapping_add(failed);
                    self.articles_per_days[idx].success =
                        self.articles_per_days[idx].success.wrapping_add(success);
                }
            }
            self.total_bytes = self.total_bytes.wrapping_add(b);
            self.custom_bytes = self.custom_bytes.wrapping_add(b);
            self.data_time = cur_time;
        }

        fn reset_custom(&mut self) {
            self.custom_bytes = 0;
            self.custom_time = current_unix_time();
        }

        fn reset(&mut self) {
            self.total_bytes = 0;
            self.data_time = 0;
            self.first_day = 0;
            self.counters_reset_time = current_unix_time();
            self.bytes_per_seconds.fill(0);
            self.bytes_per_minutes.fill(0);
            self.bytes_per_hours.fill(0);
            self.bytes_per_days.fill(0);
            self.articles_per_days.fill(NzbgetRsRpcArticlePair {
                failed: 0,
                success: 0,
            });
            self.reset_custom();
        }

        fn log_debug_info(&self, log: &NzbgetRsLog, opts: Option<&NzbgetRsOptions>) {
            log.emit_bytes(0, opts, b"   ---------- ServerVolume");
            let mut m = b"Secs: ".to_vec();
            m.extend_from_slice(&statmeter_format_indexed_bytes_line(
                0,
                &self.bytes_per_seconds,
            ));
            log.emit_bytes(0, opts, &m);
            m.clear();
            m.extend_from_slice(b"Mins: ");
            m.extend_from_slice(&statmeter_format_indexed_bytes_line(
                0,
                &self.bytes_per_minutes,
            ));
            log.emit_bytes(0, opts, &m);
            m.clear();
            m.extend_from_slice(b"Hours: ");
            m.extend_from_slice(&statmeter_format_indexed_bytes_line(
                0,
                &self.bytes_per_hours,
            ));
            log.emit_bytes(0, opts, &m);
            m.clear();
            m.extend_from_slice(b"Days: ");
            m.extend_from_slice(&statmeter_format_indexed_bytes_line(
                self.first_day,
                &self.bytes_per_days,
            ));
            log.emit_bytes(0, opts, &m);
        }
    }

    pub struct NzbgetRsStatMeter {
        speed_bytes: [AtomicI32; SPEEDMETER_SLOTS],
        speed_time: [AtomicI32; SPEEDMETER_SLOTS],
        speed_correction: AtomicI64,
        cur_sec_time: AtomicI64,
        speed_total_bytes: AtomicI64,
        speed_bytes_index: AtomicI32,
        cur_sec_bytes: AtomicI32,
        speed_start_time: AtomicI32,
        speed_mtx: Mutex<()>,

        all_bytes: AtomicI64,
        start_server: AtomicI64,
        paused_from: AtomicI64,
        start_download: AtomicI64,
        stand_by: AtomicBool,
        last_check: AtomicI64,
        last_time_offset: AtomicI64,

        stat_changed: AtomicBool,
        volumes: Mutex<Vec<RsServerVolume>>,
    }

    impl NzbgetRsStatMeter {
        fn new(log: Option<&NzbgetRsLog>, opts: Option<&NzbgetRsOptions>) -> Self {
            if let Some(lg) = log {
                lg.emit_bytes(3, opts, b"Creating StatMeter");
            }
            let sm = Self {
                speed_bytes: std::array::from_fn(|_| AtomicI32::new(0)),
                speed_time: std::array::from_fn(|_| AtomicI32::new(0)),
                speed_correction: AtomicI64::new(0),
                cur_sec_time: AtomicI64::new(0),
                speed_total_bytes: AtomicI64::new(0),
                speed_bytes_index: AtomicI32::new(0),
                cur_sec_bytes: AtomicI32::new(0),
                speed_start_time: AtomicI32::new(0),
                speed_mtx: Mutex::new(()),
                all_bytes: AtomicI64::new(0),
                start_server: AtomicI64::new(0),
                paused_from: AtomicI64::new(0),
                start_download: AtomicI64::new(0),
                stand_by: AtomicBool::new(true),
                last_check: AtomicI64::new(0),
                last_time_offset: AtomicI64::new(0),
                stat_changed: AtomicBool::new(false),
                volumes: Mutex::new(Vec::new()),
            };
            sm.reset_speed_stat();
            sm
        }

        fn reset_speed_stat(&self) {
            let cur_time = current_unix_time();
            let start = (cur_time as i32) / SPEEDMETER_SLOTSIZE - 1;
            self.speed_start_time.store(start, Ordering::SeqCst);
            for i in 0..SPEEDMETER_SLOTS {
                self.speed_bytes[i].store(0, Ordering::SeqCst);
                self.speed_time[i].store(start, Ordering::SeqCst);
            }
            self.speed_bytes_index.store(0, Ordering::SeqCst);
            self.speed_total_bytes.store(0, Ordering::SeqCst);
            self.speed_correction.store(cur_time, Ordering::SeqCst);
            self.cur_sec_time.store(0, Ordering::SeqCst);
            self.cur_sec_bytes.store(0, Ordering::SeqCst);
        }

        pub(crate) fn current_download_speed(&self) -> i64 {
            calc_current_download_speed(
                self.stand_by.load(Ordering::SeqCst),
                current_unix_time(),
                self.speed_start_time.load(Ordering::SeqCst),
                self.speed_total_bytes.load(Ordering::SeqCst),
            )
        }

        pub(crate) fn momentary_download_speed(&self) -> i64 {
            calc_momentary_download_speed(
                current_unix_time(),
                self.cur_sec_time.load(Ordering::SeqCst),
                i64::from(self.cur_sec_bytes.load(Ordering::SeqCst)),
            )
        }

        pub(crate) fn add_speed_reading_locked(&self, bytes: i32) {
            let _guard = match self.speed_mtx.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let cur_time = current_unix_time();
            let now_slot = (cur_time as i32) / SPEEDMETER_SLOTSIZE;

            if cur_time != self.cur_sec_time.load(Ordering::SeqCst) {
                self.cur_sec_time.store(cur_time, Ordering::SeqCst);
                self.cur_sec_bytes.store(0, Ordering::SeqCst);
            }
            self.cur_sec_bytes.fetch_add(bytes, Ordering::SeqCst);

            let mut idx = self
                .speed_bytes_index
                .load(Ordering::SeqCst)
                .clamp(0, (SPEEDMETER_SLOTS - 1) as i32) as usize;
            while now_slot > self.speed_time[idx].load(Ordering::SeqCst) {
                idx += 1;
                if idx >= SPEEDMETER_SLOTS {
                    idx = 0;
                }
                let old_b = i64::from(self.speed_bytes[idx].load(Ordering::SeqCst));
                self.speed_total_bytes.fetch_sub(old_b, Ordering::SeqCst);
                self.speed_start_time.store(
                    self.speed_time[idx].load(Ordering::SeqCst),
                    Ordering::SeqCst,
                );
                self.speed_bytes[idx].store(0, Ordering::SeqCst);
                self.speed_time[idx].store(now_slot, Ordering::SeqCst);
            }
            self.speed_bytes_index.store(idx as i32, Ordering::SeqCst);

            if cur_time > self.speed_correction.load(Ordering::SeqCst) {
                let mut total: i64 = 0;
                for slot in &self.speed_bytes {
                    total += i64::from(slot.load(Ordering::SeqCst));
                }
                self.speed_total_bytes.store(total, Ordering::SeqCst);
                self.speed_correction.store(cur_time, Ordering::SeqCst);
            }

            if self.speed_total_bytes.load(Ordering::SeqCst) == 0 {
                self.speed_start_time.store(now_slot, Ordering::SeqCst);
            }
            self.speed_bytes[idx].fetch_add(bytes, Ordering::SeqCst);
            self.speed_total_bytes
                .fetch_add(i64::from(bytes), Ordering::SeqCst);
            self.all_bytes.fetch_add(i64::from(bytes), Ordering::SeqCst);
        }

        fn adjust_time_offset(
            &self,
            opts: Option<&NzbgetRsOptions>,
            log: Option<&NzbgetRsLog>,
        ) -> i32 {
            let utc_time = current_unix_time();
            let t_sys = utc_time as libc::time_t;
            let mut tm_split: libc::tm = unsafe { std::mem::zeroed() };
            unsafe {
                libc::gmtime_r(&t_sys, &mut tm_split);
            }
            tm_split.tm_isdst = -1;
            let loc_time = unsafe { libc::mktime(&mut tm_split) } as i64;
            let time_corr = opts.map_or(0, |o| o.scalars().time_correction);
            let (local_delta, total_offset) = calc_local_time_offset(utc_time, loc_time, time_corr);
            self.last_time_offset.store(utc_time, Ordering::SeqCst);
            if let Some(lg) = log {
                let msg = format!("UTC delta: {total_offset} ({local_delta}+{time_corr})");
                lg.emit_bytes(3, opts, msg.as_bytes());
            }
            total_offset
        }

        fn calc_quota_usage_inner(
            &self,
            local_time_offset: i32,
            opts: Option<&NzbgetRsOptions>,
            log: Option<&NzbgetRsLog>,
        ) -> (i64, i64) {
            let (first_day, days_vec) = {
                let vols = match self.volumes.lock() {
                    Ok(g) => g,
                    Err(e) => e.into_inner(),
                };
                if let Some(v0) = vols.first() {
                    (v0.first_day, v0.bytes_per_days.clone())
                } else {
                    (0, Vec::new())
                }
            };
            let loc_time = current_unix_time().wrapping_add(i64::from(local_time_offset));
            let quota_start_day = opts.map_or(1, |o| o.scalars().quota_start_day);
            let elapsed = calc_month_slots_utc(loc_time, quota_start_day);
            let (month_bytes, day_bytes) =
                calc_quota_usage(loc_time, first_day, &days_vec, elapsed);
            if let Some(lg) = log {
                let msg = format!("month volume: {} MB", (month_bytes / 1024 / 1024) as i32);
                lg.emit_bytes(3, opts, msg.as_bytes());
            }
            (month_bytes, day_bytes)
        }

        pub(crate) fn add_server_stats(
            &self,
            bytes_read: i32,
            failed_articles: u32,
            success_articles: u32,
            server_id: i32,
            local_time_offset: i32,
        ) {
            let b_u32 = bytes_read.max(0) as u32;
            let mut vols = match self.volumes.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if let Some(v0) = vols.get_mut(0) {
                v0.add_stats(b_u32, failed_articles, success_articles, local_time_offset);
            }
            if let Some(vs) = usize::try_from(server_id)
                .ok()
                .and_then(|idx| vols.get_mut(idx))
            {
                vs.add_stats(b_u32, failed_articles, success_articles, local_time_offset);
            }
            self.stat_changed.store(true, Ordering::SeqCst);
        }
    }

    static ACTIVE_STATMETER: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsStatMeter>>> =
        std::sync::RwLock::new(None);

    pub(crate) fn active_statmeter() -> Option<std::sync::Arc<NzbgetRsStatMeter>> {
        ACTIVE_STATMETER
            .read()
            .ok()
            .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
    }

    #[no_mangle]
    pub unsafe extern "C" fn nzbget_rs_statmeter_new(
        log: *const NzbgetRsLog,
        opts: *const NzbgetRsOptions,
    ) -> *mut NzbgetRsStatMeter {
        let arc = std::sync::Arc::new(NzbgetRsStatMeter::new(log.as_ref(), opts.as_ref()));
        if let Ok(mut g) = ACTIVE_STATMETER.write() {
            *g = Some(std::sync::Arc::downgrade(&arc));
        }
        std::sync::Arc::into_raw(arc).cast_mut()
    }

    #[no_mangle]
    pub unsafe extern "C" fn nzbget_rs_statmeter_free(sm: *mut NzbgetRsStatMeter) {
        if !sm.is_null() {
            let arc = std::sync::Arc::from_raw(sm.cast_const());
            let wk = std::sync::Arc::downgrade(&arc);
            if let Ok(mut g) = ACTIVE_STATMETER.write() {
                if g.as_ref()
                    .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
                {
                    *g = None;
                }
            }
            drop(arc);
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn nzbget_rs_statmeter_speed(
        sm: *const NzbgetRsStatMeter,
        op: c_int,
        bytes: c_int,
    ) -> i64 {
        let Some(s) = sm.as_ref() else {
            return 0;
        };
        match op {
            0 => s.current_download_speed(),
            1 => s.momentary_download_speed(),
            2 => {
                s.add_speed_reading_locked(bytes);
                0
            }
            _ => 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[no_mangle]
    pub unsafe extern "C" fn nzbget_rs_statmeter_ctl(
        sm: *const NzbgetRsStatMeter,
        op: c_int,
        a1: c_int,
        a2: c_int,
        u1: u32,
        u2: u32,
        u3: u32,
        opts: *const NzbgetRsOptions,
        log: *const NzbgetRsLog,
        out_ints: *mut c_int,
        out_i64s: *mut i64,
    ) -> c_int {
        let Some(s) = sm.as_ref() else {
            return 0;
        };
        let opt_ref = opts.as_ref();
        let log_ref = log.as_ref();
        match op {
            0 => {
                let now = current_unix_time();
                s.start_server.store(now, Ordering::SeqCst);
                s.last_check.store(now, Ordering::SeqCst);
                let total_offset = s.adjust_time_offset(opt_ref, log_ref);
                if !out_ints.is_null() {
                    *out_ints = total_offset;
                }
                let mut vols = match s.volumes.lock() {
                    Ok(g) => g,
                    Err(e) => e.into_inner(),
                };
                let target_len = 1usize.saturating_add(a1.max(0) as usize);
                vols.resize_with(target_len, || RsServerVolume::new(current_unix_time()));
                1
            }
            1 => {
                let mut vols = match s.volumes.lock() {
                    Ok(g) => g,
                    Err(e) => e.into_inner(),
                };
                if let Some(v0) = vols.get_mut(0) {
                    v0.add_stats(u1, u2, u3, a2);
                }
                if let Some(vs) = usize::try_from(a1).ok().and_then(|idx| vols.get_mut(idx)) {
                    vs.add_stats(u1, u2, u3, a2);
                }
                s.stat_changed.store(true, Ordering::SeqCst);
                1
            }
            2 => {
                let cur_time = current_unix_time();
                let stand_by = s.stand_by.load(Ordering::SeqCst);
                let (up_sec, dn_sec) = calc_total_stat(
                    cur_time,
                    s.start_server.load(Ordering::SeqCst),
                    s.start_download.load(Ordering::SeqCst),
                    s.paused_from.load(Ordering::SeqCst),
                    stand_by,
                );
                if !out_ints.is_null() {
                    *out_ints.add(0) = up_sec;
                    *out_ints.add(1) = dn_sec;
                    *out_ints.add(2) = c_int::from(stand_by);
                }
                if !out_i64s.is_null() {
                    *out_i64s = s.all_bytes.load(Ordering::SeqCst);
                }
                1
            }
            3 => {
                let (month_b, day_b) = s.calc_quota_usage_inner(a1, opt_ref, log_ref);
                if !out_i64s.is_null() {
                    *out_i64s.add(0) = month_b;
                    *out_i64s.add(1) = day_b;
                }
                1
            }
            4 => {
                let cur_time = current_unix_time();
                let (adj_clk, delta, adj_dl, adj_tz) = eval_interval_check(
                    cur_time,
                    s.last_check.load(Ordering::SeqCst),
                    s.last_time_offset.load(Ordering::SeqCst),
                    s.start_download.load(Ordering::SeqCst),
                    s.stand_by.load(Ordering::SeqCst),
                );
                if adj_clk {
                    s.start_server.fetch_add(delta, Ordering::SeqCst);
                }
                if adj_dl {
                    s.start_download.fetch_add(delta, Ordering::SeqCst);
                }
                let mut mask = 0;
                let mut cur_local_offset = a1;
                if adj_tz {
                    cur_local_offset = s.adjust_time_offset(opt_ref, log_ref);
                    if !out_ints.is_null() {
                        *out_ints.add(0) = cur_local_offset;
                    }
                    mask |= 1;
                }
                s.last_check.store(cur_time, Ordering::SeqCst);

                let daily_q = opt_ref.map_or(0, |o| o.scalars().daily_quota);
                let monthly_q = opt_ref.map_or(0, |o| o.scalars().monthly_quota);
                if daily_q != 0 || monthly_q != 0 {
                    let (month_b, day_b) =
                        s.calc_quota_usage_inner(cur_local_offset, opt_ref, log_ref);
                    let (reached, action) =
                        eval_quota_status(monthly_q, daily_q, month_b, day_b, a2 != 0);
                    if let Some(lg) = log_ref {
                        match action {
                            1 => {
                                let mut m = b"Monthly quota reached at ".to_vec();
                                m.extend_from_slice(&crate::util::format_size(month_b));
                                lg.emit_bytes(1, opt_ref, &m);
                            }
                            2 => {
                                let mut m = b"Daily quota reached at ".to_vec();
                                m.extend_from_slice(&crate::util::format_size(day_b));
                                lg.emit_bytes(1, opt_ref, &m);
                            }
                            3 => lg.emit_bytes(0, opt_ref, b"Quota lifted"),
                            _ => {}
                        }
                    }
                    if !out_ints.is_null() {
                        *out_ints.add(1) = c_int::from(reached);
                    }
                    mask |= 2;
                }
                if s.stat_changed.load(Ordering::SeqCst) {
                    mask |= 4;
                }
                mask
            }
            5 => {
                let enter = a1 != 0;
                s.stand_by.store(enter, Ordering::SeqCst);
                if enter {
                    s.paused_from.store(current_unix_time(), Ordering::SeqCst);
                } else {
                    let cur = current_unix_time();
                    if s.start_download
                        .compare_exchange(0, cur, Ordering::SeqCst, Ordering::SeqCst)
                        .is_err()
                    {
                        let cur2 = current_unix_time();
                        let paused = s.paused_from.load(Ordering::SeqCst);
                        s.start_download
                            .fetch_add(cur2.wrapping_sub(paused), Ordering::SeqCst);
                    }
                    s.paused_from.store(0, Ordering::SeqCst);
                    s.reset_speed_stat();
                }
                1
            }
            6 => {
                let Some(lg) = log_ref else {
                    return 0;
                };
                lg.emit_bytes(0, opt_ref, b"   ---------- SpeedMeter");
                let spd = (s.current_download_speed() / 1024) as i32;
                lg.emit_bytes(0, opt_ref, format!("      Speed: {spd}").as_bytes());
                let sst = s.speed_start_time.load(Ordering::SeqCst);
                lg.emit_bytes(
                    0,
                    opt_ref,
                    format!("      SpeedStartTime: {sst}").as_bytes(),
                );
                let stb = s.speed_total_bytes.load(Ordering::SeqCst);
                lg.emit_bytes(
                    0,
                    opt_ref,
                    format!("      SpeedTotalBytes: {stb}").as_bytes(),
                );
                let sbi = s.speed_bytes_index.load(Ordering::SeqCst);
                lg.emit_bytes(
                    0,
                    opt_ref,
                    format!("      SpeedBytesIndex: {sbi}").as_bytes(),
                );
                let ab = s.all_bytes.load(Ordering::SeqCst);
                lg.emit_bytes(0, opt_ref, format!("      AllBytes: {ab}").as_bytes());
                let cur_int = current_unix_time() as i32;
                lg.emit_bytes(0, opt_ref, format!("      Time: {cur_int}").as_bytes());
                let tdiff = (cur_int as u32)
                    .wrapping_sub((sst as u32).wrapping_mul(SPEEDMETER_SLOTSIZE as u32))
                    as i32;
                lg.emit_bytes(0, opt_ref, format!("      TimeDiff: {tdiff}").as_bytes());
                for i in 0..SPEEDMETER_SLOTS {
                    let b = s.speed_bytes[i].load(Ordering::SeqCst);
                    let t = s.speed_time[i].load(Ordering::SeqCst);
                    lg.emit_bytes(
                        0,
                        opt_ref,
                        format!("      Bytes[{i}]: {b}, Time[{i}]: {t}").as_bytes(),
                    );
                }
                let vols = match s.volumes.lock() {
                    Ok(g) => g,
                    Err(e) => e.into_inner(),
                };
                for (index, sv) in vols.iter().enumerate() {
                    lg.emit_bytes(0, opt_ref, format!("      ServerVolume {index}").as_bytes());
                    sv.log_debug_info(lg, opt_ref);
                }
                1
            }
            _ => 0,
        }
    }

    struct StatDiskBridge<'a> {
        vols: &'a mut [RsServerVolume],
        servers: &'a [NzbgetRsServerMatchItem],
        state_ids: Vec<c_int>,
        opts: Option<&'a NzbgetRsOptions>,
        log: Option<&'a NzbgetRsLog>,
    }

    unsafe extern "C" fn stat_disk_log_cb(raw: *mut c_void, level: c_int, msg: *const c_char) {
        let br = unsafe { &*raw.cast::<StatDiskBridge<'_>>() };
        let Some(lg) = br.log else {
            return;
        };
        let bytes = if msg.is_null() {
            &[][..]
        } else {
            unsafe { CStr::from_ptr(msg) }.to_bytes()
        };
        let kind = match level {
            0 => 3, // debug
            1 => 4, // detail
            2 => 0, // info
            3 => 1, // warn
            4 => 2, // error
            _ => return,
        };
        lg.emit_bytes(kind, br.opts, bytes);
    }

    unsafe extern "C" fn stat_disk_aux_cb(
        raw: *mut c_void,
        op: c_int,
        a1: c_int,
        a2: c_int,
        _u1: u32,
        _s1: *const c_char,
        _s2: *const c_char,
        vol_item: *const c_void,
        sink_ctx: *mut c_void,
        sink_fn: Option<DiskAuxSinkFn>,
    ) {
        let br = unsafe { &mut *raw.cast::<StatDiskBridge<'_>>() };
        match op {
            4 => {
                let Some(sink) = sink_fn else {
                    return;
                };
                let srv_ptr = if br.servers.is_empty() {
                    ptr::null()
                } else {
                    br.servers.as_ptr()
                };
                if a1 == 0 || br.servers.is_empty() {
                    unsafe {
                        sink(
                            sink_ctx,
                            0,
                            0,
                            0,
                            0,
                            ptr::null(),
                            srv_ptr,
                            br.servers.len(),
                            ptr::null(),
                            0,
                        );
                    }
                    return;
                }
                let mut vol_items = Vec::with_capacity(br.vols.len());
                for sv in br.vols.iter() {
                    vol_items.push(NzbgetRsDiskVolumeStatInput {
                        first_day: sv.first_day,
                        data_time: sv.data_time as c_int,
                        custom_time: sv.custom_time as c_int,
                        counters_reset_time: sv.counters_reset_time as c_int,
                        total_bytes: sv.total_bytes,
                        custom_bytes: sv.custom_bytes,
                        bytes_per_seconds: sv.bytes_per_seconds.as_ptr(),
                        bytes_per_seconds_len: sv.bytes_per_seconds.len(),
                        bytes_per_minutes: sv.bytes_per_minutes.as_ptr(),
                        bytes_per_minutes_len: sv.bytes_per_minutes.len(),
                        bytes_per_hours: sv.bytes_per_hours.as_ptr(),
                        bytes_per_hours_len: sv.bytes_per_hours.len(),
                        bytes_per_days: sv.bytes_per_days.as_ptr(),
                        bytes_per_days_len: sv.bytes_per_days.len(),
                        articles_per_days: if sv.articles_per_days.is_empty() {
                            ptr::null()
                        } else {
                            sv.articles_per_days.as_ptr()
                        },
                        articles_per_days_len: sv.articles_per_days.len(),
                    });
                }
                unsafe {
                    sink(
                        sink_ctx,
                        0,
                        0,
                        0,
                        0,
                        ptr::null(),
                        srv_ptr,
                        br.servers.len(),
                        vol_items.as_ptr().cast(),
                        vol_items.len(),
                    );
                }
            }
            5 => {
                if let Some(slot) = usize::try_from(a1)
                    .ok()
                    .and_then(|i| br.state_ids.get_mut(i))
                {
                    *slot = a2;
                }
            }
            6 => {
                if vol_item.is_null() {
                    return;
                }
                let vi = unsafe { &*vol_item.cast::<NzbgetRsDiskVolumeStatInput>() };
                let mut target_idx: Option<usize> = None;
                if a1 == 0 {
                    if !br.vols.is_empty() {
                        target_idx = Some(0);
                    }
                } else {
                    for (i, srv) in br.servers.iter().enumerate() {
                        if br.state_ids.get(i).copied() == Some(a1) {
                            if let Ok(vid) = usize::try_from(srv.id) {
                                if vid < br.vols.len() {
                                    target_idx = Some(vid);
                                }
                            }
                        }
                    }
                }
                let Some(vidx) = target_idx else {
                    return;
                };
                let sv = &mut br.vols[vidx];
                let format_ver = a2 >> 1;
                if (a2 & 1) != 0 {
                    sv.custom_time = i64::from(vi.custom_time);
                    sv.counters_reset_time = i64::from(vi.counters_reset_time);
                }
                sv.first_day = vi.first_day;
                sv.data_time = i64::from(vi.data_time);
                sv.total_bytes = vi.total_bytes;
                sv.custom_bytes = vi.custom_bytes;
                let copy_i64 = |ptr_in: *const i64, len: usize| -> Vec<i64> {
                    if ptr_in.is_null() || len == 0 {
                        Vec::new()
                    } else {
                        unsafe { std::slice::from_raw_parts(ptr_in, len) }.to_vec()
                    }
                };
                sv.bytes_per_seconds = copy_i64(vi.bytes_per_seconds, vi.bytes_per_seconds_len);
                sv.bytes_per_minutes = copy_i64(vi.bytes_per_minutes, vi.bytes_per_minutes_len);
                sv.bytes_per_hours = copy_i64(vi.bytes_per_hours, vi.bytes_per_hours_len);
                sv.bytes_per_days = copy_i64(vi.bytes_per_days, vi.bytes_per_days_len);
                if format_ver >= 4 {
                    if vi.articles_per_days.is_null() || vi.articles_per_days_len == 0 {
                        sv.articles_per_days = Vec::new();
                    } else {
                        sv.articles_per_days = unsafe {
                            std::slice::from_raw_parts(
                                vi.articles_per_days,
                                vi.articles_per_days_len,
                            )
                        }
                        .to_vec();
                    }
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[no_mangle]
    pub unsafe extern "C" fn nzbget_rs_statmeter_disk(
        sm: *const NzbgetRsStatMeter,
        op: c_int,
        server_mode: c_int,
        local_time_offset: c_int,
        opts: *const NzbgetRsOptions,
        log: *const NzbgetRsLog,
        pool: *const crate::net::NzbgetRsServerPool,
        inout_perfect: *mut c_int,
    ) -> c_int {
        let Some(s) = sm.as_ref() else {
            return 0;
        };
        let opt_ref = opts.as_ref();
        let log_ref = log.as_ref();
        if op == 0 && (server_mode == 0 || opt_ref.is_none()) {
            return 0;
        }
        let pool_srvs = pool
            .as_ref()
            .map_or_else(Vec::new, |p| p.servers_snapshot());
        let srv_items: Vec<NzbgetRsServerMatchItem> = pool_srvs
            .iter()
            .map(|ns| NzbgetRsServerMatchItem {
                id: ns.id,
                port: ns.port,
                name: ns.name.as_ptr(),
                host: ns.host.as_ptr(),
                user: ns.user.as_ptr(),
            })
            .collect();
        let mut vols = match s.volumes.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        let mut bridge = StatDiskBridge {
            vols: &mut vols,
            servers: &srv_items,
            state_ids: vec![-1; srv_items.len()],
            opts: opt_ref,
            log: log_ref,
        };
        let mut pm = if !inout_perfect.is_null() {
            *inout_perfect
        } else {
            1
        };
        let mut ds_env: NzbgetRsDiskStateEnv = std::mem::zeroed();
        ds_env.ctx = (&mut bridge as *mut StatDiskBridge<'_>).cast();
        ds_env.queue_dir = opt_ref
            .and_then(|o| o.str_cstr(6))
            .map_or(ptr::null(), CStr::as_ptr);
        ds_env.temp_dir = opt_ref
            .and_then(|o| o.str_cstr(5))
            .map_or(ptr::null(), CStr::as_ptr);
        ds_env.line_ending = c"\n".as_ptr();
        ds_env.sep = c_int::from(crate::paths::PATH_SEPARATOR as u8);
        ds_env.has_options = c_int::from(opt_ref.is_some());
        ds_env.flush_queue = opt_ref.map_or(0, |o| o.scalars().flush_queue);
        ds_env.out_flag = if op == 1 && !inout_perfect.is_null() {
            &mut pm
        } else {
            ptr::null_mut()
        };
        ds_env.log_cb = Some(stat_disk_log_cb);
        ds_env.aux_cb = Some(stat_disk_aux_cb);
        match op {
            0 => {
                let ok = diskstate_exec(11, &ds_env);
                s.stat_changed.store(false, Ordering::SeqCst);
                ok
            }
            1 => {
                let ok = diskstate_exec(12, &ds_env);
                for (ns, &st_id) in pool_srvs.iter().zip(bridge.state_ids.iter()) {
                    if st_id >= 0 {
                        ns.state_id.store(st_id, Ordering::SeqCst);
                    }
                }
                if !inout_perfect.is_null() {
                    *inout_perfect = pm;
                }
                let off = i64::from(local_time_offset);
                for sv in vols.iter_mut() {
                    let loc_t = sv.data_time.wrapping_add(off);
                    sv.calc_slots(loc_t);
                }
                ok
            }
            _ => 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[no_mangle]
    pub unsafe extern "C" fn nzbget_rs_statmeter_rpc(
        sm: *const NzbgetRsStatMeter,
        op: c_int,
        local_time_offset: c_int,
        a1: c_int,
        a2: c_int,
        a3: c_int,
        a4: c_int,
        a5: c_int,
        emit_ctx: *mut c_void,
        emit_cb: Option<NzbgetRsXmlRpcEmitFn>,
    ) -> c_int {
        let Some(s) = sm.as_ref() else {
            return 0;
        };
        let mut vols = match s.volumes.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        for sv in vols.iter_mut() {
            sv.add_stats(0, 0, 0, local_time_offset);
        }
        match op {
            0 => {
                let mut ok = false;
                for (idx, sv) in vols.iter_mut().enumerate() {
                    if (idx as c_int) == a1 || a1 == -1 {
                        if a2 == 0 {
                            sv.reset();
                        } else {
                            sv.reset_custom();
                        }
                        ok = true;
                    }
                }
                if ok {
                    s.stat_changed.store(true, Ordering::SeqCst);
                }
                c_int::from(ok)
            }
            1 => {
                let Some(cb) = emit_cb else {
                    return 0;
                };
                for (index, sv) in vols.iter().enumerate() {
                    let art_ptr = if a5 != 0 && !sv.articles_per_days.is_empty() {
                        sv.articles_per_days.as_ptr()
                    } else {
                        ptr::null()
                    };
                    let art_len = if a5 != 0 {
                        sv.articles_per_days.len()
                    } else {
                        0
                    };
                    let vf = NzbgetRsRpcServerVolumeItem {
                        server_id: index as c_int,
                        data_time: sv.data_time as c_int,
                        first_day: sv.first_day,
                        total_bytes: sv.total_bytes,
                        custom_bytes: sv.custom_bytes,
                        custom_time: sv.custom_time as c_int,
                        counters_reset_time: sv.counters_reset_time as c_int,
                        sec_slot: sv.sec_slot,
                        min_slot: sv.min_slot,
                        hour_slot: sv.hour_slot,
                        day_slot: sv.day_slot,
                        include_seconds: a1,
                        bytes_per_seconds: sv.bytes_per_seconds.as_ptr(),
                        bytes_per_seconds_len: sv.bytes_per_seconds.len(),
                        include_minutes: a2,
                        bytes_per_minutes: sv.bytes_per_minutes.as_ptr(),
                        bytes_per_minutes_len: sv.bytes_per_minutes.len(),
                        include_hours: a3,
                        bytes_per_hours: sv.bytes_per_hours.as_ptr(),
                        bytes_per_hours_len: sv.bytes_per_hours.len(),
                        include_days: a4,
                        bytes_per_days: sv.bytes_per_days.as_ptr(),
                        bytes_per_days_len: sv.bytes_per_days.len(),
                        include_articles: a5,
                        articles_per_days: art_ptr,
                        articles_per_days_len: art_len,
                    };
                    cb(
                        emit_ctx,
                        14,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        (&vf as *const NzbgetRsRpcServerVolumeItem).cast(),
                        0,
                        ptr::null(),
                        0,
                        ptr::null(),
                        0,
                        ptr::null(),
                        0,
                    );
                }
                1
            }
            _ => 0,
        }
    }
}

#[cfg(unix)]
pub(crate) use runtime::active_statmeter;
#[cfg(unix)]
pub use runtime::{
    nzbget_rs_statmeter_ctl, nzbget_rs_statmeter_disk, nzbget_rs_statmeter_free,
    nzbget_rs_statmeter_new, nzbget_rs_statmeter_rpc, nzbget_rs_statmeter_speed, NzbgetRsStatMeter,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots() {
        let mut first = 0;
        // 2026-10-10 12:34:56
        let s = calc_slots(1_791_635_696, &mut first);
        assert_eq!(
            (s.sec, s.min, s.hour, s.day, s.in_range),
            (56, 34, 12, 0, 1)
        );
        assert_eq!(first, 1_791_635_696 / 86400);
        let s = calc_slots(1_791_635_696 + 86400 * 3, &mut first);
        assert_eq!(s.day, 3);
        assert_eq!(calc_slots(1000, &mut first).day, -1);
    }

    #[test]
    fn clears_and_adds() {
        let (mut sec, mut min, mut hour) = (vec![7i64; 60], vec![7i64; 60], vec![7i64; 24]);
        let mut first = 0;
        let s = calc_slots(1_791_635_696, &mut first);
        add_stats(
            &mut sec,
            &mut min,
            &mut hour,
            &s,
            s.min,
            s.hour,
            1_791_635_696,
            1_791_635_696 - 3,
            5,
        );
        assert_eq!(&sec[53..57], &[7, 0, 0, 5]);
        assert_eq!(min[34], 12);
        // negative slots (an int time before 1970) are skipped, not written
        let s = calc_slots(-61, &mut first);
        add_stats(&mut sec, &mut min, &mut hour, &s, 0, 0, -61, 0, 1);
    }

    #[test]
    fn test_statmeter_functions() {
        let (adj_clk, delta, adj_dl, adj_tz) = eval_interval_check(200, 100, 100, 50, false);
        assert!(adj_clk);
        assert_eq!(delta, 101);
        assert!(adj_dl);
        assert!(adj_tz);

        let (up, dn) = calc_total_stat(1000, 900, 950, 980, true);
        assert_eq!(up, 100);
        assert_eq!(dn, 30);

        assert_eq!(calc_current_download_speed(false, 110, 100, 5000), 500);
        assert_eq!(calc_current_or_momentary(), 1024);

        let mut cur_sec_time = 0i64;
        let mut cur_sec_bytes = 0i64;
        let mut speed_bytes = [0i32; SPEEDMETER_SLOTS];
        let mut speed_time = [100i32; SPEEDMETER_SLOTS];
        let mut idx = 0i32;
        let mut total = 0i64;
        let mut start = 100i32;
        let mut corr = 100i64;
        let mut all = 0i64;
        add_speed_reading(
            4096,
            105,
            &mut cur_sec_time,
            &mut cur_sec_bytes,
            &mut speed_bytes,
            &mut speed_time,
            &mut idx,
            &mut total,
            &mut start,
            &mut corr,
            &mut all,
        );
        assert_eq!(cur_sec_time, 105);
        assert_eq!(cur_sec_bytes, 4096);
        assert_eq!(total, 4096);
        assert_eq!(all, 4096);

        let days = [100i64, 200, 300, 400];
        let (month_b, day_b) = calc_quota_usage(86400 * 13, 10, &days, 3);
        assert_eq!(day_b, 400);
        assert_eq!(month_b, 400 + 300 + 200);

        let (reached, action) = eval_quota_status(1, 0, 2 * 1024 * 1024, 0, false);
        assert!(reached);
        assert_eq!(action, 1);
    }

    fn calc_current_or_momentary() -> i64 {
        calc_momentary_download_speed(50, 50, 1024)
    }

    #[test]
    fn server_volume_test_port_from_cpp() {
        let mut sec_bytes = vec![0i64; 60];
        let mut min_bytes = vec![0i64; 60];
        let mut hour_bytes = vec![0i64; 24];
        let mut day_bytes: Vec<i64> = Vec::new();
        let mut day_articles: Vec<(i32, i32)> = Vec::new();
        let mut first_day = 0i32;
        let mut data_time = 0i64;
        let mut prev_min = -1;
        let mut prev_hour = -1;
        let mut total_bytes = 0i64;
        let mut custom_bytes = 0i64;

        let now = 1_791_635_696i64;
        for &(bytes, failed, success) in &[(100i64, 10i32, 20i32), (200i64, 30i32, 40i32)] {
            let slots = calc_slots(now, &mut first_day);
            let req = server_volume_required_day_len(slots.day, day_bytes.len());
            if req > 0 {
                day_bytes.resize(req, 0);
                day_articles.resize(req, (0, 0));
            }
            add_stats(
                &mut sec_bytes,
                &mut min_bytes,
                &mut hour_bytes,
                &slots,
                prev_min,
                prev_hour,
                now,
                data_time,
                bytes,
            );
            prev_min = slots.min;
            prev_hour = slots.hour;
            data_time = now;
            let day_idx = slots.day as usize;
            day_bytes[day_idx] += bytes;
            day_articles[day_idx].0 += failed;
            day_articles[day_idx].1 += success;
            total_bytes += bytes;
            custom_bytes += bytes;
        }

        let slots1 = calc_slots(now, &mut first_day);
        assert_eq!(day_bytes.len(), 1);
        assert_eq!(day_articles[slots1.day as usize], (40, 60));
        assert_eq!(sec_bytes[slots1.sec as usize], 300);
        assert_eq!(min_bytes[slots1.min as usize], 300);
        assert_eq!(hour_bytes[slots1.hour as usize], 300);
        assert_eq!(day_bytes[slots1.day as usize], 300);
        assert_eq!(total_bytes, 300);
        assert_eq!(custom_bytes, 300);

        let next_day = now + 86400;
        let slots2 = calc_slots(next_day, &mut first_day);
        let req2 = server_volume_required_day_len(slots2.day, day_bytes.len());
        if req2 > 0 {
            day_bytes.resize(req2, 0);
            day_articles.resize(req2, (0, 0));
        }
        assert_eq!(slots2.day, slots1.day + 1);
        assert_eq!(day_bytes.len(), 2);
        assert_eq!(day_articles.len(), 2);

        custom_bytes = 0;
        assert_eq!(custom_bytes, 0);
        assert_eq!(sec_bytes[slots1.sec as usize], 300);
        assert_eq!(min_bytes[slots1.min as usize], 300);
        assert_eq!(hour_bytes[slots1.hour as usize], 300);
        assert_eq!(day_bytes[slots1.day as usize], 300);
        assert_eq!(day_bytes[slots2.day as usize], 0);
        assert_eq!(day_articles[slots1.day as usize], (40, 60));
        assert_eq!(day_articles[slots2.day as usize], (0, 0));
        assert_eq!(total_bytes, 300);

        // 2025-03-05 12:00:00 UTC -> 20152 days * 86400 + 43200 = 1_741_176_000
        let mar_5_2025 = 1_741_176_000i64;
        assert_eq!(calc_month_slots_utc(mar_5_2025, 1), 5);
        assert_eq!(calc_month_slots_utc(mar_5_2025, 5), 1);
        assert_eq!(calc_month_slots_utc(mar_5_2025, 20), (28 - 20) + 5 + 1);
        assert_eq!(calc_month_slots_utc(mar_5_2025, 31), 5);
    }

    #[test]
    fn volume_extreme_times_and_wide_counters() {
        let mut first = 17;
        let mut sec = [91, 1i64 << 40, 92];
        let mut min = [93, 7, 94];
        let mut hour = [95, 8, 96];
        let mut slots = Slots {
            sec: 0,
            min: -1,
            hour: 1,
            ..Slots::default()
        };
        add_stats(
            &mut sec[1..2],
            &mut min[1..2],
            &mut hour[1..2],
            &slots,
            0,
            0,
            0,
            0,
            5,
        );
        assert_eq!(sec, [91, (1i64 << 40) + 5, 92]);
        assert_eq!(min, [93, 7, 94]);
        assert_eq!(hour, [95, 8, 96]);
        for t in [i64::MIN, i64::MAX, i64::from(i32::MIN), -2_147_483_647, -61] {
            slots = calc_slots(t, &mut first);
            add_stats(
                &mut [],
                &mut sec[1..2],
                &mut [],
                &slots,
                0,
                0,
                t,
                0,
                i64::MAX,
            );
        }
        assert_eq!((sec[0], sec[2]), (91, 92));
        assert_eq!(hour, [95, 8, 96]);
    }

    #[cfg(unix)]
    #[test]
    fn statmeter_runtime_disk_and_rpc_roundtrip() {
        use crate::log::{nzbget_rs_log_free, nzbget_rs_log_new};
        use crate::options::{
            nzbget_rs_options_free, nzbget_rs_options_new, NzbgetRsParsedOptions,
        };
        use crate::xmlrpc_format::NzbgetRsRpcServerVolumeItem;
        use std::ffi::{c_char, c_int, c_void, CString};
        use std::ptr;

        let dir =
            std::env::temp_dir().join(format!("rustyget_statmeter_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let qdir = dir.join("queue");
        let tdir = dir.join("tmp");
        let _ = std::fs::create_dir_all(&qdir);
        let _ = std::fs::create_dir_all(&tdir);

        let q_opt = CString::new(format!("QueueDir={}", qdir.to_str().unwrap())).unwrap();
        let t_opt = CString::new(format!("TempDir={}", tdir.to_str().unwrap())).unwrap();
        let cmd = [
            q_opt.as_ptr(),
            t_opt.as_ptr(),
            c"WriteLog=none".as_ptr(),
            c"-s".as_ptr(),
        ];
        let mut scalars = NzbgetRsParsedOptions::default();
        let mut flags = 0;
        unsafe {
            let log = nzbget_rs_log_new(0);
            let opts = nzbget_rs_options_new(
                c"rustyget".as_ptr(),
                ptr::null(),
                1,
                1,
                cmd.as_ptr(),
                cmd.len(),
                0,
                c"1.0".as_ptr(),
                ptr::null_mut(),
                None,
                &mut scalars,
                &mut flags,
            );
            let sm = nzbget_rs_statmeter_new(log, opts);
            let mut init_off = 0;
            nzbget_rs_statmeter_ctl(
                sm,
                0,
                1,
                0,
                0,
                0,
                0,
                opts,
                log,
                &mut init_off,
                ptr::null_mut(),
            );
            nzbget_rs_statmeter_ctl(
                sm,
                5,
                0,
                0,
                0,
                0,
                0,
                opts,
                log,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            nzbget_rs_statmeter_speed(sm, 2, 8192);
            nzbget_rs_statmeter_ctl(
                sm,
                1,
                1,
                init_off,
                4096,
                1,
                7,
                opts,
                log,
                ptr::null_mut(),
                ptr::null_mut(),
            );

            let mut tot_ints = [0i32; 3];
            let mut all_b = 0i64;
            nzbget_rs_statmeter_ctl(
                sm,
                2,
                0,
                0,
                0,
                0,
                0,
                opts,
                log,
                tot_ints.as_mut_ptr(),
                &mut all_b,
            );
            assert_eq!(tot_ints[2], 0);
            assert_eq!(all_b, 8192);

            let pool = crate::net::nzbget_rs_serverpool_new(None);
            crate::net::nzbget_rs_serverpool_add(
                pool,
                1,
                1,
                c"News1".as_ptr(),
                c"news.example.com".as_ptr(),
                563,
                0,
                c"alice".as_ptr(),
                c"".as_ptr(),
                0,
                1,
                c"".as_ptr(),
                1,
                0,
                0,
                0,
                0,
                0,
            );
            assert_eq!(
                nzbget_rs_statmeter_disk(sm, 0, 1, init_off, opts, log, pool, ptr::null_mut()),
                1
            );

            let sm2 = nzbget_rs_statmeter_new(log, opts);
            nzbget_rs_statmeter_ctl(
                sm2,
                0,
                1,
                0,
                0,
                0,
                0,
                opts,
                log,
                &mut init_off,
                ptr::null_mut(),
            );
            let mut perfect = 0i32;
            assert_eq!(
                nzbget_rs_statmeter_disk(sm2, 1, 1, init_off, opts, log, pool, &mut perfect),
                1
            );
            assert_eq!(perfect, 1);
            assert_eq!(
                (*pool)
                    .server_by_id(1)
                    .unwrap()
                    .state_id
                    .load(std::sync::atomic::Ordering::SeqCst),
                1
            );
            crate::net::nzbget_rs_serverpool_free(pool);

            unsafe extern "C" fn capture_vols(
                ctx: *mut c_void,
                kind: c_int,
                _a1: c_int,
                _a2: c_int,
                _a3: c_int,
                _a4: c_int,
                _a5: c_int,
                _a6: c_int,
                _i1: i64,
                _i2: i64,
                _i3: i64,
                _s1: *const c_char,
                _s2: *const c_char,
                _s3: *const c_char,
                _s4: *const c_char,
                p1: *const c_void,
                _len1: usize,
                _p2: *const c_void,
                _len2: usize,
                _p3: *const c_void,
                _len3: usize,
                _p4: *const c_void,
                _len4: usize,
            ) {
                assert_eq!(kind, 14);
                let vf = &*(p1 as *const NzbgetRsRpcServerVolumeItem);
                let out = &mut *(ctx as *mut Vec<(i32, i64, i64)>);
                out.push((vf.server_id, vf.total_bytes, vf.custom_bytes));
            }

            let mut vols_out: Vec<(i32, i64, i64)> = Vec::new();
            assert_eq!(
                nzbget_rs_statmeter_rpc(
                    sm2,
                    1,
                    init_off,
                    1,
                    1,
                    1,
                    1,
                    1,
                    (&mut vols_out as *mut Vec<_>).cast(),
                    Some(capture_vols),
                ),
                1
            );
            assert_eq!(vols_out, vec![(0, 4096, 4096), (1, 4096, 4096)]);

            // Custom reset on server 1 only
            assert_eq!(
                nzbget_rs_statmeter_rpc(sm2, 0, init_off, 1, 1, 0, 0, 0, ptr::null_mut(), None),
                1
            );
            vols_out.clear();
            nzbget_rs_statmeter_rpc(
                sm2,
                1,
                init_off,
                0,
                0,
                0,
                0,
                0,
                (&mut vols_out as *mut Vec<_>).cast(),
                Some(capture_vols),
            );
            assert_eq!(vols_out, vec![(0, 4096, 4096), (1, 4096, 0)]);

            nzbget_rs_statmeter_free(sm2);
            nzbget_rs_statmeter_free(sm);
            nzbget_rs_options_free(opts);
            nzbget_rs_log_free(log);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
