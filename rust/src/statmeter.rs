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
    seconds: &mut [i64], minutes: &mut [i64], hours: &mut [i64], s: &Slots, last_min_slot: i32, last_hour_slot: i32,
    loc_cur_time: i64, loc_data_time: i64, bytes: i64,
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
pub fn calc_local_time_offset(
    utc_time: i64,
    loc_time: i64,
    time_correction: i32,
) -> (i32, i32) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots() {
        let mut first = 0;
        // 2026-10-10 12:34:56
        let s = calc_slots(1_791_635_696, &mut first);
        assert_eq!((s.sec, s.min, s.hour, s.day, s.in_range), (56, 34, 12, 0, 1));
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
        add_stats(&mut sec, &mut min, &mut hour, &s, s.min, s.hour, 1_791_635_696, 1_791_635_696 - 3, 5);
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
}
